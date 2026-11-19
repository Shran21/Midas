//! DXGI desktop duplication for every attached output. Losing access to an output, as happens on a
//! desktop switch such as a UAC prompt, counts as a desktop change.

use std::{io, time::Duration};

use windows::{
    Win32::{
        Foundation::{E_ACCESSDENIED, HMODULE},
        Graphics::{
            Direct3D::D3D_DRIVER_TYPE_UNKNOWN,
            Direct3D11::{
                D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAP_READ,
                D3D11_MAPPED_SUBRESOURCE, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC,
                D3D11_USAGE_STAGING, D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext,
                ID3D11Texture2D,
            },
            Dxgi::{
                Common::{
                    DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_MODE_ROTATION_ROTATE90,
                    DXGI_MODE_ROTATION_ROTATE180, DXGI_MODE_ROTATION_ROTATE270, DXGI_SAMPLE_DESC,
                },
                CreateDXGIFactory1, DXGI_ERROR_ACCESS_LOST, DXGI_ERROR_NOT_FOUND,
                DXGI_ERROR_WAIT_TIMEOUT, DXGI_OUTDUPL_FRAME_INFO, IDXGIAdapter1, IDXGIFactory1,
                IDXGIOutput1, IDXGIOutputDuplication, IDXGIResource,
            },
        },
    },
    core::Interface,
};

use crate::capture::{self, Frame, Monitor, Rect, Rotation, Selection};

struct Output {
    monitor: Monitor,
    adapter: IDXGIAdapter1,
    output: IDXGIOutput1,
    device: Option<(ID3D11Device, ID3D11DeviceContext)>,
    duplication: Option<IDXGIOutputDuplication>,
    staging: Option<(ID3D11Texture2D, u32, u32)>,
    frame: Option<Frame>,
}

enum Acquired {
    Fresh,
    Unchanged,
    /// The output cannot be duplicated now, for example while the secure desktop is shown.
    Unavailable,
}

fn failure(error: windows::core::Error) -> io::Error {
    io::Error::other(error)
}

/// The attached outputs of every adapter, with their desktop positions.
pub fn monitors() -> io::Result<Vec<Monitor>> {
    Ok(outputs()?
        .into_iter()
        .map(|output| output.monitor)
        .collect())
}

fn outputs() -> io::Result<Vec<Output>> {
    // SAFETY: creates a new factory object owned by the returned interface.
    let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1() }.map_err(failure)?;
    let mut found = Vec::new();
    for adapter_index in 0.. {
        // SAFETY: enumeration on a live factory; NOT_FOUND ends the list.
        let adapter = match unsafe { factory.EnumAdapters1(adapter_index) } {
            Ok(adapter) => adapter,
            Err(error) if error.code() == DXGI_ERROR_NOT_FOUND => break,
            Err(error) => return Err(failure(error)),
        };
        for output_index in 0.. {
            // SAFETY: enumeration on a live adapter; NOT_FOUND ends the list.
            let output = match unsafe { adapter.EnumOutputs(output_index) } {
                Ok(output) => output,
                Err(error) if error.code() == DXGI_ERROR_NOT_FOUND => break,
                Err(error) => return Err(failure(error)),
            };
            // SAFETY: reads the descriptor of a live output.
            let desc = unsafe { output.GetDesc() }.map_err(failure)?;
            if !desc.AttachedToDesktop.as_bool() {
                continue;
            }
            let area = desc.DesktopCoordinates;
            let (Ok(width), Ok(height)) = (
                u32::try_from(area.right - area.left),
                u32::try_from(area.bottom - area.top),
            ) else {
                continue;
            };
            let name_end = desc
                .DeviceName
                .iter()
                .position(|&unit| unit == 0)
                .unwrap_or(desc.DeviceName.len());
            let monitor = Monitor {
                id: String::from_utf16_lossy(&desc.DeviceName[..name_end]),
                bounds: Rect {
                    x: area.left,
                    y: area.top,
                    width,
                    height,
                },
                primary: area.left == 0 && area.top == 0,
                rotation: match desc.Rotation {
                    DXGI_MODE_ROTATION_ROTATE90 => Rotation::Rotate90,
                    DXGI_MODE_ROTATION_ROTATE180 => Rotation::Rotate180,
                    DXGI_MODE_ROTATION_ROTATE270 => Rotation::Rotate270,
                    _ => Rotation::Identity,
                },
            };
            found.push(Output {
                monitor,
                adapter: adapter.clone(),
                output: output.cast().map_err(failure)?,
                device: None,
                duplication: None,
                staging: None,
                frame: None,
            });
        }
    }
    Ok(found)
}

impl Output {
    fn duplication(&mut self) -> windows::core::Result<IDXGIOutputDuplication> {
        if let Some(duplication) = &self.duplication {
            return Ok(duplication.clone());
        }
        if self.device.is_none() {
            let (mut device, mut context) = (None, None);
            // SAFETY: all out pointers are valid; the device is created on this output's adapter.
            unsafe {
                D3D11CreateDevice(
                    &self.adapter,
                    D3D_DRIVER_TYPE_UNKNOWN,
                    HMODULE::default(),
                    D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                    None,
                    D3D11_SDK_VERSION,
                    Some(&mut device),
                    None,
                    Some(&mut context),
                )?;
            }
            self.device = device.zip(context);
        }
        let (device, _) = self
            .device
            .as_ref()
            .ok_or_else(|| windows::core::Error::from(E_ACCESSDENIED))?;
        // SAFETY: duplicates this live output for a device on the same adapter.
        let duplication = unsafe { self.output.DuplicateOutput(device) }?;
        self.duplication = Some(duplication.clone());
        Ok(duplication)
    }

    fn acquire(&mut self, wait: Duration) -> io::Result<Acquired> {
        let duplication = match self.duplication() {
            Ok(duplication) => duplication,
            Err(error)
                if error.code() == E_ACCESSDENIED || error.code() == DXGI_ERROR_ACCESS_LOST =>
            {
                return Ok(Acquired::Unavailable);
            }
            Err(error) => return Err(failure(error)),
        };
        let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
        let mut resource: Option<IDXGIResource> = None;
        let wait = u32::try_from(wait.as_millis()).unwrap_or(u32::MAX);
        // SAFETY: both out pointers are valid for the call; a successful acquire is released below.
        match unsafe { duplication.AcquireNextFrame(wait, &mut info, &mut resource) } {
            Ok(()) => {}
            Err(error) if error.code() == DXGI_ERROR_WAIT_TIMEOUT => {
                return Ok(Acquired::Unchanged);
            }
            Err(error) if error.code() == DXGI_ERROR_ACCESS_LOST => {
                self.duplication = None;
                return Ok(Acquired::Unavailable);
            }
            Err(error) => return Err(failure(error)),
        }
        let copied = self.copy(info, resource);
        // SAFETY: releases the frame acquired above exactly once.
        let released = unsafe { duplication.ReleaseFrame() };
        if let Err(error) = released
            && error.code() == DXGI_ERROR_ACCESS_LOST
        {
            self.duplication = None;
        }
        copied
    }

    fn copy(
        &mut self,
        info: DXGI_OUTDUPL_FRAME_INFO,
        resource: Option<IDXGIResource>,
    ) -> io::Result<Acquired> {
        // Only the pointer moved; the desktop image is unchanged.
        if info.LastPresentTime == 0 {
            return Ok(Acquired::Unchanged);
        }
        let texture: ID3D11Texture2D = resource
            .ok_or_else(|| io::Error::other("no desktop image"))?
            .cast()
            .map_err(failure)?;
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        // SAFETY: reads the descriptor of the acquired texture into a valid structure.
        unsafe { texture.GetDesc(&mut desc) };
        if desc.Format != DXGI_FORMAT_B8G8R8A8_UNORM {
            return Err(io::Error::other("unexpected desktop image format"));
        }
        let (device, context) = self
            .device
            .clone()
            .ok_or_else(|| io::Error::other("no device"))?;
        if self
            .staging
            .as_ref()
            .is_none_or(|(_, width, height)| (*width, *height) != (desc.Width, desc.Height))
        {
            let staging_desc = D3D11_TEXTURE2D_DESC {
                Width: desc.Width,
                Height: desc.Height,
                MipLevels: 1,
                ArraySize: 1,
                Format: desc.Format,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Usage: D3D11_USAGE_STAGING,
                BindFlags: 0,
                CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                MiscFlags: 0,
            };
            let mut staging = None;
            // SAFETY: the descriptor is valid and the out pointer receives a new texture.
            unsafe { device.CreateTexture2D(&staging_desc, None, Some(&mut staging)) }
                .map_err(failure)?;
            let staging = staging.ok_or_else(|| io::Error::other("no staging texture"))?;
            self.staging = Some((staging, desc.Width, desc.Height));
        }
        let (staging, width, height) = self
            .staging
            .clone()
            .ok_or_else(|| io::Error::other("no staging texture"))?;
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        // SAFETY: copies between same-sized textures of one device, then maps the CPU-readable copy.
        unsafe {
            context.CopyResource(&staging, &texture);
            context
                .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                .map_err(failure)?;
        }
        let row = width as usize * 4;
        let mut bgra = self
            .frame
            .take()
            .map(|frame| frame.bgra)
            .unwrap_or_default();
        bgra.resize(row * height as usize, 0);
        for line in 0..height as usize {
            // SAFETY: the mapping holds `height` rows of `RowPitch` bytes, each at least `row` long.
            let source = unsafe {
                std::slice::from_raw_parts(
                    mapped
                        .pData
                        .cast::<u8>()
                        .add(line * mapped.RowPitch as usize),
                    row,
                )
            };
            bgra[line * row..(line + 1) * row].copy_from_slice(source);
        }
        // SAFETY: unmaps the subresource mapped above.
        unsafe { context.Unmap(&staging, 0) };
        self.frame = Some(Frame {
            width,
            height,
            bgra,
        });
        Ok(Acquired::Fresh)
    }
}

/// Captures the selected monitor, or every monitor laid out as the virtual desktop.
pub struct Duplicator {
    view: Rect,
    outputs: Vec<Output>,
    available: bool,
    generation: u64,
    frame: Frame,
}

impl Duplicator {
    pub fn open(selection: &Selection) -> io::Result<Self> {
        let all = outputs()?;
        let monitors: Vec<Monitor> = all.iter().map(|output| output.monitor.clone()).collect();
        let view = capture::view(&monitors, selection)
            .map_err(|_| io::Error::new(io::ErrorKind::NotFound, "no such monitor"))?;
        let selected = capture::sources(&monitors, selection)
            .into_iter()
            .map(|monitor| monitor.id.clone())
            .collect::<Vec<_>>();
        Ok(Self {
            view,
            outputs: all
                .into_iter()
                .filter(|output| selected.contains(&output.monitor.id))
                .collect(),
            available: true,
            generation: 0,
            frame: Frame::blank(view.width, view.height),
        })
    }

    pub fn view(&self) -> Rect {
        self.view
    }

    /// Changes whenever the duplicated desktop was lost or came back.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// The newest image of the view, or `None` while the desktop cannot be duplicated.
    pub fn capture(&mut self, wait: Duration) -> io::Result<Option<&Frame>> {
        let mut fresh = false;
        let mut available = true;
        let per_output = wait / u32::try_from(self.outputs.len().max(1)).unwrap_or(1);
        for output in &mut self.outputs {
            match output.acquire(per_output)? {
                Acquired::Fresh => fresh = true,
                Acquired::Unchanged => {}
                Acquired::Unavailable => available = false,
            }
        }
        if available != self.available {
            self.available = available;
            self.generation += 1;
        }
        if !available {
            return Ok(None);
        }
        if fresh {
            for output in &self.outputs {
                if let Some(frame) = &output.frame {
                    capture::compose(self.view, &output.monitor, frame, &mut self.frame)
                        .map_err(|_| io::Error::other("desktop image size changed"))?;
                }
            }
        }
        Ok(Some(&self.frame))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Desktop duplication needs an interactive desktop on a WDDM display, which remote and
    /// service sessions often lack; the test checks the real path only where Windows offers it.
    #[test]
    fn the_real_desktop_is_described_and_captured_where_available() {
        let monitors = match monitors() {
            Ok(monitors) if !monitors.is_empty() => monitors,
            other => {
                eprintln!("no attached display output: {other:?}");
                return;
            }
        };
        assert!(monitors.iter().all(|monitor| monitor.bounds.width > 0));
        let mut duplicator = match Duplicator::open(&Selection::Combined) {
            Ok(duplicator) => duplicator,
            Err(error) => {
                eprintln!("desktop duplication unavailable: {error}");
                return;
            }
        };
        let view = duplicator.view();
        match duplicator.capture(Duration::from_millis(500)) {
            Ok(Some(frame)) => {
                assert_eq!((frame.width, frame.height), (view.width, view.height));
            }
            Ok(None) => eprintln!("the desktop cannot be duplicated in this session"),
            Err(error) => eprintln!("desktop duplication failed: {error}"),
        }
    }
}
