//! Hardware H.264 through the vendor encoders that graphics drivers register with Media
//! Foundation: Intel Quick Sync, NVIDIA NVENC and AMD AMF. No vendor SDK ships with the agent.
//! These encoders are asynchronous: they ask for input and announce output with events, which are
//! polled here without blocking, so a stuck encoder costs a bounded wait and never the session.

use std::{
    collections::VecDeque,
    io,
    mem::ManuallyDrop,
    time::{Duration, Instant},
};

use windows::{
    Win32::{
        Media::MediaFoundation::{
            CODECAPI_AVEncCommonMeanBitRate, CODECAPI_AVEncVideoForceKeyFrame, ICodecAPI,
            IMFActivate, IMFMediaEventGenerator, IMFSample, IMFTransform, METransformHaveOutput,
            METransformNeedInput, MF_E_NO_EVENTS_AVAILABLE, MF_E_TRANSFORM_STREAM_CHANGE,
            MF_EVENT_FLAG_NO_WAIT, MF_EVENT_TYPE, MF_TRANSFORM_ASYNC_UNLOCK, MFCreateMemoryBuffer,
            MFCreateSample, MFMediaType_Video, MFT_CATEGORY_VIDEO_ENCODER, MFT_ENUM_FLAG_HARDWARE,
            MFT_ENUM_FLAG_SORTANDFILTER, MFT_ENUM_HARDWARE_VENDOR_ID_Attribute,
            MFT_FRIENDLY_NAME_Attribute, MFT_MESSAGE_NOTIFY_BEGIN_STREAMING,
            MFT_MESSAGE_NOTIFY_START_OF_STREAM, MFT_OUTPUT_DATA_BUFFER,
            MFT_OUTPUT_STREAM_PROVIDES_SAMPLES, MFT_REGISTER_TYPE_INFO, MFTEnumEx,
            MFVideoFormat_H264, MFVideoFormat_NV12,
        },
        System::{Com::CoTaskMemFree, Variant::VARIANT},
    },
    core::{Interface, PWSTR},
};

use midas_agent_core::media::frame::nv12_len;

use super::{configure, failed, platform};

/// How long a frame may wait for the encoder to take it.
const INPUT_WAIT: Duration = Duration::from_millis(500);
/// How long a frame waits for its own output before the next one comes; at most one frame.
const OUTPUT_WAIT: Duration = Duration::from_millis(40);
/// Frames whose output is still awaited, for the latency measurement.
const INFLIGHT: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Vendor {
    Intel,
    Nvidia,
    Amd,
    Other,
}

impl Vendor {
    /// From the PCI vendor id the driver registers, such as `VEN_8086`.
    pub fn from_id(id: &str) -> Self {
        match id.to_ascii_uppercase().as_str() {
            "VEN_8086" => Self::Intel,
            "VEN_10DE" => Self::Nvidia,
            "VEN_1002" | "VEN_1022" => Self::Amd,
            _ => Self::Other,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Intel => "intel",
            Self::Nvidia => "nvidia",
            Self::Amd => "amd",
            Self::Other => "other",
        }
    }
}

/// A hardware encoder Windows offers, best first as Media Foundation sorts them.
pub struct Candidate {
    pub name: String,
    pub vendor: Vendor,
    activate: IMFActivate,
}

/// # Safety
/// `activate` must be a live activation object.
unsafe fn text(activate: &IMFActivate, key: &windows::core::GUID) -> Option<String> {
    let mut value = PWSTR::null();
    let mut length = 0;
    // SAFETY: the string is allocated for the caller, read, then freed.
    unsafe {
        activate
            .GetAllocatedString(key, &mut value, &mut length)
            .ok()?;
        let read = value.to_string().ok();
        CoTaskMemFree(Some(value.0.cast()));
        read
    }
}

/// The hardware H.264 encoders that take NV12; empty where there are none.
pub fn candidates() -> io::Result<Vec<Candidate>> {
    platform()?;
    let input = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: MFVideoFormat_NV12,
    };
    let output = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: MFVideoFormat_H264,
    };
    let mut found: *mut Option<IMFActivate> = std::ptr::null_mut();
    let mut count = 0;
    // SAFETY: the type descriptions outlive the call; the returned array is read, then freed.
    unsafe {
        MFTEnumEx(
            MFT_CATEGORY_VIDEO_ENCODER,
            MFT_ENUM_FLAG_HARDWARE | MFT_ENUM_FLAG_SORTANDFILTER,
            Some(&input),
            Some(&output),
            &mut found,
            &mut count,
        )
        .map_err(failed)?;
        if found.is_null() {
            return Ok(Vec::new());
        }
        let candidates = (0..count as usize)
            .filter_map(|index| std::ptr::read(found.add(index)))
            .map(|activate| Candidate {
                name: text(&activate, &MFT_FRIENDLY_NAME_Attribute)
                    .unwrap_or_else(|| "hardware H.264".into()),
                vendor: text(&activate, &MFT_ENUM_HARDWARE_VENDOR_ID_Attribute)
                    .map_or(Vendor::Other, |id| Vendor::from_id(&id)),
                activate,
            })
            .collect();
        CoTaskMemFree(Some(found.cast()));
        Ok(candidates)
    }
}

pub struct HardwareH264 {
    transform: IMFTransform,
    codec: ICodecAPI,
    events: IMFMediaEventGenerator,
    activate: IMFActivate,
    width: u32,
    height: u32,
    duration: i64,
    time: i64,
    /// Inputs the encoder asked for and has not received yet.
    credits: u32,
    inflight: VecDeque<(i64, Instant)>,
    latency: Option<Duration>,
    output: Vec<u8>,
    pub name: String,
    pub vendor: Vendor,
}

impl HardwareH264 {
    pub fn new(
        candidate: &Candidate,
        width: u32,
        height: u32,
        fps: u32,
        bitrate: u32,
    ) -> io::Result<Self> {
        if width == 0
            || height == 0
            || !width.is_multiple_of(2)
            || !height.is_multiple_of(2)
            || fps == 0
        {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        platform()?;
        // SAFETY: COM objects are used through their typed wrappers on this thread only.
        unsafe {
            let transform: IMFTransform = candidate.activate.ActivateObject().map_err(failed)?;
            // An asynchronous encoder works only once its caller says it handles the events.
            transform
                .GetAttributes()
                .map_err(failed)?
                .SetUINT32(&MF_TRANSFORM_ASYNC_UNLOCK, 1)
                .map_err(failed)?;
            let codec: ICodecAPI = transform.cast().map_err(failed)?;
            let events: IMFMediaEventGenerator = transform.cast().map_err(failed)?;
            configure(&transform, &codec, width, height, fps, bitrate)?;
            transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)
                .map_err(failed)?;
            transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)
                .map_err(failed)?;
            Ok(Self {
                transform,
                codec,
                events,
                activate: candidate.activate.clone(),
                width,
                height,
                duration: 10_000_000 / i64::from(fps),
                time: 0,
                credits: 0,
                inflight: VecDeque::new(),
                latency: None,
                output: Vec::new(),
                name: candidate.name.clone(),
                vendor: candidate.vendor,
            })
        }
    }

    pub fn set_bitrate(&mut self, bitrate: u32) -> io::Result<()> {
        // SAFETY: a documented dynamic encoder property on the live encoder.
        unsafe {
            self.codec
                .SetValue(&CODECAPI_AVEncCommonMeanBitRate, &VARIANT::from(bitrate))
                .map_err(failed)
        }
    }

    /// From a frame's input to its encoded output, as last measured.
    pub fn latency(&self) -> Option<Duration> {
        self.latency
    }

    /// Encodes one NV12 frame of the configured size into an Annex B byte stream. The output may
    /// belong to an earlier frame, and is empty while the encoder still works on it.
    pub fn encode(&mut self, nv12: &[u8], keyframe: bool) -> io::Result<&[u8]> {
        if nv12.len() != nv12_len(self.width, self.height) {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        self.output.clear();
        let deadline = Instant::now() + INPUT_WAIT;
        loop {
            self.events()?;
            if self.credits > 0 {
                break;
            }
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "the hardware encoder took no input",
                ));
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        // SAFETY: the buffer is locked only while copying `nv12.len()` bytes into its own
        // allocation of that size; the samples are owned COM objects.
        unsafe {
            if keyframe {
                self.codec
                    .SetValue(&CODECAPI_AVEncVideoForceKeyFrame, &VARIANT::from(1u32))
                    .map_err(failed)?;
            }
            let buffer = MFCreateMemoryBuffer(nv12.len() as u32).map_err(failed)?;
            let mut data = std::ptr::null_mut();
            buffer.Lock(&mut data, None, None).map_err(failed)?;
            std::ptr::copy_nonoverlapping(nv12.as_ptr(), data, nv12.len());
            buffer.Unlock().map_err(failed)?;
            buffer.SetCurrentLength(nv12.len() as u32).map_err(failed)?;
            let sample = MFCreateSample().map_err(failed)?;
            sample.AddBuffer(&buffer).map_err(failed)?;
            sample.SetSampleTime(self.time).map_err(failed)?;
            sample.SetSampleDuration(self.duration).map_err(failed)?;
            self.transform.ProcessInput(0, &sample, 0).map_err(failed)?;
        }
        self.credits -= 1;
        if self.inflight.len() == INFLIGHT {
            self.inflight.pop_front();
        }
        self.inflight.push_back((self.time, Instant::now()));
        self.time += self.duration;
        // A low-latency encoder usually finishes the frame within a few milliseconds.
        let until =
            Instant::now() + OUTPUT_WAIT.min(Duration::from_micros(self.duration as u64 / 10));
        while self.output.is_empty() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(1));
            self.events()?;
        }
        Ok(&self.output)
    }

    /// Handles every event the encoder has queued, without waiting.
    fn events(&mut self) -> io::Result<()> {
        loop {
            // SAFETY: the event generator of the live encoder; the event is an owned object.
            let event = match unsafe { self.events.GetEvent(MF_EVENT_FLAG_NO_WAIT) } {
                Ok(event) => event,
                Err(error) if error.code() == MF_E_NO_EVENTS_AVAILABLE => return Ok(()),
                Err(error) => return Err(failed(error)),
            };
            // SAFETY: as above.
            let kind = MF_EVENT_TYPE(unsafe { event.GetType() }.map_err(failed)? as i32);
            if kind == METransformNeedInput {
                self.credits += 1;
            } else if kind == METransformHaveOutput {
                self.collect()?;
            }
        }
    }

    /// Takes the output the encoder announced.
    fn collect(&mut self) -> io::Result<()> {
        // SAFETY: the caller holds the encoder; output buffers are owned and released here.
        unsafe {
            let info = self.transform.GetOutputStreamInfo(0).map_err(failed)?;
            let provided = info.dwFlags & MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32 != 0;
            let ours = if provided {
                None
            } else {
                let sample = MFCreateSample().map_err(failed)?;
                let buffer = MFCreateMemoryBuffer(info.cbSize.max(1 << 16)).map_err(failed)?;
                sample.AddBuffer(&buffer).map_err(failed)?;
                Some(sample)
            };
            let mut buffers = [MFT_OUTPUT_DATA_BUFFER {
                dwStreamID: 0,
                pSample: ManuallyDrop::new(ours),
                dwStatus: 0,
                pEvents: ManuallyDrop::new(None),
            }];
            let mut status = 0;
            let result = self.transform.ProcessOutput(0, &mut buffers, &mut status);
            let [buffer] = buffers;
            let sample: Option<IMFSample> = ManuallyDrop::into_inner(buffer.pSample);
            drop(ManuallyDrop::into_inner(buffer.pEvents));
            match result {
                Ok(()) => {}
                // The encoder settled its output format; it announces the output again.
                Err(error) if error.code() == MF_E_TRANSFORM_STREAM_CHANGE => {
                    let changed = self
                        .transform
                        .GetOutputAvailableType(0, 0)
                        .map_err(failed)?;
                    self.transform
                        .SetOutputType(0, &changed, 0)
                        .map_err(failed)?;
                    return Ok(());
                }
                Err(error) => return Err(failed(error)),
            }
            let Some(sample) = sample else {
                return Ok(());
            };
            if let Ok(time) = sample.GetSampleTime()
                && let Some(index) = self.inflight.iter().position(|(at, _)| *at == time)
            {
                let (_, started) = self.inflight.remove(index).expect("index just found");
                self.inflight.drain(..index);
                self.latency = Some(started.elapsed());
            }
            let contiguous = sample.ConvertToContiguousBuffer().map_err(failed)?;
            let mut data = std::ptr::null_mut();
            let mut length = 0;
            contiguous
                .Lock(&mut data, None, Some(&mut length))
                .map_err(failed)?;
            self.output
                .extend_from_slice(std::slice::from_raw_parts(data, length as usize));
            contiguous.Unlock().map_err(failed)?;
        }
        Ok(())
    }
}

impl Drop for HardwareH264 {
    fn drop(&mut self) {
        // SAFETY: releases the encoder this value activated; it is not used afterwards.
        unsafe {
            let _ = self.activate.ShutdownObject();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vendors_are_named_by_their_pci_id() {
        assert_eq!(Vendor::from_id("VEN_8086"), Vendor::Intel);
        assert_eq!(Vendor::from_id("ven_10de"), Vendor::Nvidia);
        assert_eq!(Vendor::from_id("VEN_1002"), Vendor::Amd);
        assert_eq!(Vendor::from_id("VEN_1022"), Vendor::Amd);
        assert_eq!(Vendor::from_id("VEN_1414"), Vendor::Other);
    }

    #[test]
    fn a_hardware_encoder_produces_h264_with_a_measured_latency() {
        let Ok(found) = candidates() else {
            println!("skipped: Media Foundation unavailable");
            return;
        };
        let Some(candidate) = found.first() else {
            println!("skipped: no hardware H.264 encoder");
            return;
        };
        println!(
            "hardware encoder: {} ({:?})",
            candidate.name, candidate.vendor
        );
        let (width, height) = (640, 360);
        let mut encoder = HardwareH264::new(candidate, width, height, 30, 1_000_000)
            .expect("the hardware encoder starts");
        let mut produced = Vec::new();
        for index in 0..30u32 {
            let mut frame = vec![128u8; nv12_len(width, height)];
            for (offset, value) in frame.iter_mut().take((width * height) as usize).enumerate() {
                *value = (16 + (offset as u32 + index * 5) % 200) as u8;
            }
            produced.extend_from_slice(encoder.encode(&frame, index == 0).unwrap());
        }
        assert!(
            produced.windows(4).any(|w| w == [0, 0, 0, 1]),
            "an Annex B stream"
        );
        let latency = encoder.latency().expect("a measured latency");
        assert!(latency < Duration::from_secs(1), "{latency:?}");
    }
}
