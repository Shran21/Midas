//! Software H.264 through the Media Foundation encoder that ships with Windows. No codec binary
//! is distributed with the agent; Windows N editions need the Media Feature Pack and Windows
//! Server the Media Foundation feature, otherwise the encoder reports itself unavailable.

use std::{io, mem::ManuallyDrop};

use windows::{
    Win32::{
        Media::MediaFoundation::{
            CLSID_MSH264EncoderMFT, CODECAPI_AVEncCommonMeanBitRate,
            CODECAPI_AVEncCommonRateControlMode, CODECAPI_AVEncMPVGOPSize,
            CODECAPI_AVEncVideoForceKeyFrame, CODECAPI_AVLowLatencyMode, ICodecAPI, IMFMediaType,
            IMFSample, IMFTransform, MF_E_TRANSFORM_NEED_MORE_INPUT, MF_E_TRANSFORM_STREAM_CHANGE,
            MF_MT_AVG_BITRATE, MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE, MF_MT_INTERLACE_MODE,
            MF_MT_MAJOR_TYPE, MF_MT_MPEG2_PROFILE, MF_MT_SUBTYPE, MF_MT_VIDEO_NOMINAL_RANGE,
            MF_MT_YUV_MATRIX, MF_VERSION, MFCreateMediaType, MFCreateMemoryBuffer, MFCreateSample,
            MFMediaType_Video, MFNominalRange_16_235, MFSTARTUP_LITE, MFStartup,
            MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, MFT_MESSAGE_NOTIFY_START_OF_STREAM,
            MFT_OUTPUT_DATA_BUFFER, MFT_OUTPUT_STREAM_PROVIDES_SAMPLES, MFVideoFormat_H264,
            MFVideoFormat_NV12, MFVideoInterlace_Progressive, MFVideoTransferMatrix_BT709,
            eAVEncCommonRateControlMode_CBR, eAVEncH264VProfile_Base,
        },
        System::{
            Com::{CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx},
            Variant::VARIANT,
        },
    },
    core::Interface,
};
use windows_sys::Win32::System::LibraryLoader::{LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW};

use midas_agent_core::media::frame::nv12_len;

fn failed(error: windows::core::Error) -> io::Error {
    io::Error::other(error)
}

/// Media Foundation is delay-loaded; load it explicitly first, so a missing component is an
/// error instead of a failed delay-load.
fn platform() -> io::Result<()> {
    let name = super::wide("mfplat.dll");
    // SAFETY: loads a system DLL by name from System32 only; the module stays loaded.
    if unsafe {
        LoadLibraryExW(
            name.as_ptr(),
            std::ptr::null_mut(),
            LOAD_LIBRARY_SEARCH_SYSTEM32,
        )
    }
    .is_null()
    {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Media Foundation is not installed",
        ));
    }
    // SAFETY: plain initialization calls; repeated calls are reference counted by COM and MF.
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED)
            .ok()
            .map_err(failed)?;
        MFStartup(MF_VERSION, MFSTARTUP_LITE).map_err(failed)
    }
}

fn pack(high: u32, low: u32) -> u64 {
    (u64::from(high) << 32) | u64::from(low)
}

/// Low-latency constant-bitrate constrained baseline from NV12; the same for every encoder.
///
/// # Safety
/// `transform` and `codec` must be one live encoder, used from this thread.
unsafe fn configure(
    transform: &IMFTransform,
    codec: &ICodecAPI,
    width: u32,
    height: u32,
    fps: u32,
    bitrate: u32,
) -> io::Result<()> {
    // SAFETY: the caller's encoder; every value passed is owned for the call.
    unsafe {
        // Encoder properties go before the media types, which fix the encoder's buffering.
        codec
            .SetValue(&CODECAPI_AVLowLatencyMode, &VARIANT::from(true))
            .map_err(failed)?;
        codec
            .SetValue(
                &CODECAPI_AVEncCommonRateControlMode,
                &VARIANT::from(eAVEncCommonRateControlMode_CBR.0 as u32),
            )
            .map_err(failed)?;
        codec
            .SetValue(&CODECAPI_AVEncCommonMeanBitRate, &VARIANT::from(bitrate))
            .map_err(failed)?;
        // Keyframes come on request after loss; a regular one every ten seconds bounds recovery.
        codec
            .SetValue(
                &CODECAPI_AVEncMPVGOPSize,
                &VARIANT::from(fps.saturating_mul(10)),
            )
            .map_err(failed)?;
        let output: IMFMediaType = MFCreateMediaType().map_err(failed)?;
        output
            .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
            .map_err(failed)?;
        output
            .SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)
            .map_err(failed)?;
        output
            .SetUINT32(&MF_MT_AVG_BITRATE, bitrate)
            .map_err(failed)?;
        output
            .SetUINT64(&MF_MT_FRAME_SIZE, pack(width, height))
            .map_err(failed)?;
        output
            .SetUINT64(&MF_MT_FRAME_RATE, pack(fps, 1))
            .map_err(failed)?;
        output
            .SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)
            .map_err(failed)?;
        output
            .SetUINT32(&MF_MT_MPEG2_PROFILE, eAVEncH264VProfile_Base.0 as u32)
            .map_err(failed)?;
        output
            .SetUINT32(&MF_MT_YUV_MATRIX, MFVideoTransferMatrix_BT709.0 as u32)
            .map_err(failed)?;
        output
            .SetUINT32(&MF_MT_VIDEO_NOMINAL_RANGE, MFNominalRange_16_235.0 as u32)
            .map_err(failed)?;
        transform.SetOutputType(0, &output, 0).map_err(failed)?;
        let input: IMFMediaType = MFCreateMediaType().map_err(failed)?;
        input
            .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
            .map_err(failed)?;
        input
            .SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12)
            .map_err(failed)?;
        input
            .SetUINT64(&MF_MT_FRAME_SIZE, pack(width, height))
            .map_err(failed)?;
        input
            .SetUINT64(&MF_MT_FRAME_RATE, pack(fps, 1))
            .map_err(failed)?;
        input
            .SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)
            .map_err(failed)?;
        transform.SetInputType(0, &input, 0).map_err(failed)?;
        Ok(())
    }
}

pub struct SoftwareH264 {
    transform: IMFTransform,
    codec: ICodecAPI,
    width: u32,
    height: u32,
    duration: i64,
    time: i64,
    output: Vec<u8>,
}

impl SoftwareH264 {
    /// A constrained-baseline, low-latency, constant-bitrate encoder for NV12 frames.
    pub fn new(width: u32, height: u32, fps: u32, bitrate: u32) -> io::Result<Self> {
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
            let transform: IMFTransform =
                CoCreateInstance(&CLSID_MSH264EncoderMFT, None, CLSCTX_INPROC_SERVER).map_err(
                    |_| io::Error::new(io::ErrorKind::Unsupported, "no Windows H.264 encoder"),
                )?;
            let codec: ICodecAPI = transform.cast().map_err(failed)?;
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
                width,
                height,
                duration: 10_000_000 / i64::from(fps),
                time: 0,
                output: Vec::new(),
            })
        }
    }

    /// Changes the target bitrate without restarting the stream.
    pub fn set_bitrate(&mut self, bitrate: u32) -> io::Result<()> {
        // SAFETY: a documented dynamic encoder property on the live encoder.
        unsafe {
            self.codec
                .SetValue(&CODECAPI_AVEncCommonMeanBitRate, &VARIANT::from(bitrate))
                .map_err(failed)
        }
    }

    /// Encodes one NV12 frame of the configured size; the result is an Annex B byte stream,
    /// empty while the encoder still buffers.
    pub fn encode(&mut self, nv12: &[u8], keyframe: bool) -> io::Result<&[u8]> {
        if nv12.len() != nv12_len(self.width, self.height) {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        self.output.clear();
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
            self.time += self.duration;
            self.transform.ProcessInput(0, &sample, 0).map_err(failed)?;
            while let Some(sample) = self.next()? {
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
        }
        Ok(&self.output)
    }

    /// One encoded sample, or `None` once the encoder needs more input.
    unsafe fn next(&mut self) -> io::Result<Option<IMFSample>> {
        // SAFETY: the caller holds the encoder; output buffers are owned and released here.
        unsafe {
            loop {
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
                let sample = ManuallyDrop::into_inner(buffer.pSample);
                drop(ManuallyDrop::into_inner(buffer.pEvents));
                match result {
                    Ok(()) => return Ok(sample),
                    Err(error) if error.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => {
                        return Ok(None);
                    }
                    Err(error) if error.code() == MF_E_TRANSFORM_STREAM_CHANGE => {
                        let changed = self
                            .transform
                            .GetOutputAvailableType(0, 0)
                            .map_err(failed)?;
                        self.transform
                            .SetOutputType(0, &changed, 0)
                            .map_err(failed)?;
                    }
                    Err(error) => return Err(failed(error)),
                }
            }
        }
    }
}

pub mod hardware;

#[cfg(test)]
mod tests;
