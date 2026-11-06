/// One captured frame in BGRA byte order.
pub struct Frame {
    pub width: usize,
    pub height: usize,
    pub bgra: Vec<u8>,
}

pub trait Source {
    fn name(&self) -> &'static str;
    /// Returns the newest frame, repeating the previous one when the screen did not change.
    fn capture(&mut self) -> Option<&Frame>;
    /// Generation and name of the desktop the next capture reads; the generation changes on every desktop switch.
    fn surface(&self) -> (u64, &'static str) {
        (0, "default")
    }
}

/// Moving test pattern for builds without desktop duplication.
pub struct Synthetic {
    frame: Frame,
    tick: usize,
}

impl Synthetic {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            frame: Frame {
                width,
                height,
                bgra: vec![0; width * height * 4],
            },
            tick: 0,
        }
    }
}

impl Source for Synthetic {
    fn name(&self) -> &'static str {
        "synthetic"
    }

    fn capture(&mut self) -> Option<&Frame> {
        self.tick += 1;
        let (w, h, t) = (self.frame.width, self.frame.height, self.tick);
        for (i, px) in self
            .frame
            .bgra
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .enumerate()
        {
            let (x, y) = (i % w, i / w);
            let stripe = ((x / 8 + y / 24) % 2) as u8 * 40;
            px[0] = ((x + t * 4) % 256) as u8;
            px[1] = ((y * 255 / h) as u8).wrapping_add(stripe);
            px[2] = (((x + y + t * 2) / 4) % 256) as u8;
            px[3] = 255;
        }
        Some(&self.frame)
    }
}

#[cfg(windows)]
pub mod desktop {
    use win_desktop_duplication::{
        DesktopDuplicationApi, co_init, devices::AdapterFactory, set_process_dpi_awareness,
        tex_reader::TextureReader,
    };

    use super::{Frame, Source};

    /// Real Windows desktop through DXGI Desktop Duplication.
    pub struct Desktop {
        duplication: DesktopDuplicationApi,
        reader: TextureReader,
        frame: Option<Frame>,
    }

    impl Desktop {
        pub fn open() -> Result<Self, String> {
            set_process_dpi_awareness();
            co_init();
            let adapter = AdapterFactory::new()
                .get_adapter_by_idx(0)
                .ok_or("no graphics adapter")?;
            let output = adapter.get_display_by_idx(0).ok_or("no display output")?;
            let duplication = DesktopDuplicationApi::new(adapter, output)
                .map_err(|e| format!("desktop duplication unavailable: {e:?}"))?;
            let (device, context) = duplication.get_device_and_ctx();
            Ok(Self {
                duplication,
                reader: TextureReader::new(device, context),
                frame: None,
            })
        }
    }

    impl Source for Desktop {
        fn name(&self) -> &'static str {
            "dxgi-desktop-duplication"
        }

        fn capture(&mut self) -> Option<&Frame> {
            if let Ok(texture) = self.duplication.acquire_next_frame_now() {
                let desc = texture.desc();
                let mut bgra = self.frame.take().map(|f| f.bgra).unwrap_or_default();
                if self.reader.get_data(&mut bgra, &texture).is_ok() {
                    self.frame = Some(Frame {
                        width: desc.width as usize,
                        height: desc.height as usize,
                        bgra,
                    });
                }
            }
            self.frame.as_ref()
        }
    }
}

/// Windows Graphics Capture; works where DXGI duplication is denied, for example in RDP sessions.
#[cfg(windows)]
pub mod graphics {
    use std::sync::{Arc, Mutex};

    use windows_capture::{
        capture::{CaptureControl, Context, GraphicsCaptureApiHandler},
        frame::Frame as CapturedFrame,
        graphics_capture_api::InternalCaptureControl,
        monitor::Monitor,
        settings::{
            ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
            MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
        },
    };

    use super::{Frame, Source};

    type Shared = Arc<Mutex<Option<Frame>>>;
    type Failure = Box<dyn std::error::Error + Send + Sync>;

    pub struct Handler {
        shared: Shared,
        scratch: Vec<u8>,
    }

    impl GraphicsCaptureApiHandler for Handler {
        type Flags = Shared;
        type Error = Failure;

        fn new(context: Context<Self::Flags>) -> Result<Self, Self::Error> {
            Ok(Self {
                shared: context.flags,
                scratch: Vec::new(),
            })
        }

        fn on_frame_arrived(
            &mut self,
            frame: &mut CapturedFrame,
            _control: InternalCaptureControl,
        ) -> Result<(), Self::Error> {
            let (width, height) = (frame.width() as usize, frame.height() as usize);
            let buffer = frame.buffer()?;
            let bgra = buffer.as_nopadding_buffer(&mut self.scratch).to_vec();
            if let Ok(mut slot) = self.shared.lock() {
                *slot = Some(Frame {
                    width,
                    height,
                    bgra,
                });
            }
            Ok(())
        }
    }

    pub struct Graphics {
        shared: Shared,
        current: Option<Frame>,
        _control: CaptureControl<Handler, Failure>,
    }

    impl Graphics {
        pub fn open() -> Result<Self, String> {
            let shared: Shared = Arc::default();
            let monitor = Monitor::primary().map_err(|e| format!("no primary monitor: {e:?}"))?;
            let settings = Settings::new(
                monitor,
                CursorCaptureSettings::WithCursor,
                DrawBorderSettings::WithoutBorder,
                SecondaryWindowSettings::Default,
                MinimumUpdateIntervalSettings::Default,
                DirtyRegionSettings::Default,
                ColorFormat::Bgra8,
                shared.clone(),
            );
            let control = Handler::start_free_threaded(settings)
                .map_err(|e| format!("graphics capture unavailable: {e:?}"))?;
            Ok(Self {
                shared,
                current: None,
                _control: control,
            })
        }
    }

    impl Source for Graphics {
        fn name(&self) -> &'static str {
            "windows-graphics-capture"
        }

        fn capture(&mut self) -> Option<&Frame> {
            if let Ok(mut slot) = self.shared.lock()
                && let Some(frame) = slot.take()
            {
                self.current = Some(frame);
            }
            self.current.as_ref()
        }
    }
}

pub fn open() -> Box<dyn Source> {
    #[cfg(windows)]
    {
        match desktop::Desktop::open() {
            Ok(desktop) => return Box::new(desktop),
            Err(error) => tracing::warn!(%error, "trying Windows Graphics Capture"),
        }
        match graphics::Graphics::open() {
            Ok(graphics) => return Box::new(graphics),
            Err(error) => tracing::warn!(%error, "falling back to the synthetic source"),
        }
    }
    Box::new(Synthetic::new(1280, 720))
}
