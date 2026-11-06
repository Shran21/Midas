use std::{ptr, sync::Arc};

use midas_media_prototype::capture::{Frame, Source};
use windows_sys::Win32::{
    Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CAPTUREBLT, CreateCompatibleDC,
        CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, HBITMAP, HDC, HGDIOBJ,
        ReleaseDC, SRCCOPY, SelectObject,
    },
    UI::WindowsAndMessaging::{
        CURSOR_SHOWING, CURSORINFO, DI_NORMAL, DrawIconEx, GetCursorInfo, GetIconInfo,
        GetSystemMetrics, ICONINFO, SM_CXSCREEN, SM_CYSCREEN,
    },
};

use super::desktop::{Attachment, Kind, Tracker};

fn primary_size() -> (i32, i32) {
    // SAFETY: plain metric queries.
    unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) }
}

/// GDI device contexts for the primary screen of the desktop the thread is attached to.
struct Canvas {
    screen: HDC,
    memory: HDC,
    bitmap: HBITMAP,
    previous: HGDIOBJ,
    bits: *const u8,
    width: i32,
    height: i32,
}

impl Canvas {
    fn new() -> Option<Self> {
        let (width, height) = primary_size();
        if width <= 0 || height <= 0 {
            return None;
        }
        // SAFETY: every handle is checked; Drop releases them in reverse order.
        unsafe {
            let screen = GetDC(ptr::null_mut());
            if screen.is_null() {
                return None;
            }
            let memory = CreateCompatibleDC(screen);
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width,
                    biHeight: -height,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits = ptr::null_mut();
            let bitmap = if memory.is_null() {
                ptr::null_mut()
            } else {
                CreateDIBSection(memory, &info, DIB_RGB_COLORS, &mut bits, ptr::null_mut(), 0)
            };
            if bitmap.is_null() || bits.is_null() {
                if !memory.is_null() {
                    DeleteDC(memory);
                }
                ReleaseDC(ptr::null_mut(), screen);
                return None;
            }
            let previous = SelectObject(memory, bitmap);
            Some(Self {
                screen,
                memory,
                bitmap,
                previous,
                bits: bits.cast(),
                width,
                height,
            })
        }
    }

    fn grab(&self, frame: &mut Frame) -> bool {
        // SAFETY: both contexts are valid and the copy stays inside the bitmap.
        let copied = unsafe {
            BitBlt(
                self.memory,
                0,
                0,
                self.width,
                self.height,
                self.screen,
                0,
                0,
                SRCCOPY | CAPTUREBLT,
            )
        };
        if copied == 0 {
            return false;
        }
        self.draw_cursor();
        let length = self.width as usize * self.height as usize * 4;
        // SAFETY: the 32-bit top-down section holds exactly `length` bytes while the bitmap lives.
        let pixels = unsafe { std::slice::from_raw_parts(self.bits, length) };
        frame.width = self.width as usize;
        frame.height = self.height as usize;
        frame.bgra.clear();
        frame.bgra.extend_from_slice(pixels);
        true
    }

    /// GDI copies do not include the pointer; the technician needs it on the prompt.
    fn draw_cursor(&self) {
        let mut cursor = CURSORINFO {
            cbSize: size_of::<CURSORINFO>() as u32,
            ..Default::default()
        };
        let mut icon = ICONINFO::default();
        // SAFETY: the structures are initialised; GetIconInfo's bitmaps are deleted here.
        unsafe {
            if GetCursorInfo(&mut cursor) == 0
                || cursor.flags & CURSOR_SHOWING == 0
                || GetIconInfo(cursor.hCursor, &mut icon) == 0
            {
                return;
            }
            DrawIconEx(
                self.memory,
                cursor.ptScreenPos.x - icon.xHotspot as i32,
                cursor.ptScreenPos.y - icon.yHotspot as i32,
                cursor.hCursor,
                0,
                0,
                0,
                ptr::null_mut(),
                DI_NORMAL,
            );
            for bitmap in [icon.hbmMask, icon.hbmColor] {
                if !bitmap.is_null() {
                    DeleteObject(bitmap);
                }
            }
        }
    }
}

impl Drop for Canvas {
    fn drop(&mut self) {
        // SAFETY: releases exactly what `new` acquired.
        unsafe {
            SelectObject(self.memory, self.previous);
            DeleteObject(self.bitmap);
            DeleteDC(self.memory);
            ReleaseDC(ptr::null_mut(), self.screen);
        }
    }
}

/// Captures whichever desktop currently receives input, including the secure desktop under LocalSystem.
pub struct Screen {
    tracker: Arc<Tracker>,
    attachment: Attachment,
    attached: Option<u64>,
    canvas: Option<Canvas>,
    frame: Frame,
}

impl Screen {
    pub fn new(tracker: Arc<Tracker>) -> Self {
        Self {
            tracker,
            attachment: Attachment::new(),
            attached: None,
            canvas: None,
            frame: Frame {
                width: 1280,
                height: 720,
                bgra: Vec::new(),
            },
        }
    }

    fn blank(&mut self) {
        let length = self.frame.width * self.frame.height * 4;
        self.frame.bgra.clear();
        self.frame.bgra.resize(length, 0x30);
    }
}

impl Source for Screen {
    fn name(&self) -> &'static str {
        "gdi-input-desktop"
    }

    fn capture(&mut self) -> Option<&Frame> {
        let (generation, _) = self.tracker.current();
        if self.attached != Some(generation) {
            self.canvas = None;
            self.attached = Some(generation);
            if self.attachment.follow() != Kind::Inaccessible {
                self.canvas = Canvas::new();
            }
        } else if self
            .canvas
            .as_ref()
            .is_some_and(|c| (c.width, c.height) != primary_size())
        {
            self.canvas = Canvas::new();
        }
        let grabbed = match &self.canvas {
            Some(canvas) => canvas.grab(&mut self.frame),
            None => false,
        };
        if !grabbed {
            self.blank();
        }
        Some(&self.frame)
    }

    fn surface(&self) -> (u64, &'static str) {
        let (generation, kind) = self.tracker.current();
        (generation, kind.name())
    }
}
