//! Query the frontmost (focused) Windows application.
//!
//! Resolves the foreground window's owning process to an executable path via
//! `GetForegroundWindow` → `GetWindowThreadProcessId` → `OpenProcess` →
//! `QueryFullProcessImageNameW`. There is no bundle-id concept on Windows, so
//! `FrontmostApp::name` is the executable's file name (e.g. "notepad.exe") and
//! `bundle_id` carries the full image path. `None` if the chain fails.

use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, HWND, MAX_PATH};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

use crate::FrontmostApp;

#[must_use]
pub fn frontmost_app() -> Option<FrontmostApp> {
    // SAFETY: all calls below operate on handles obtained within this function;
    // the process handle is closed before returning.
    unsafe {
        let hwnd: HWND = GetForegroundWindow();
        if hwnd.0.is_null() {
            return None;
        }

        let mut pid: u32 = 0;
        let thread_id = GetWindowThreadProcessId(hwnd, Some(&raw mut pid));
        if thread_id == 0 || pid == 0 {
            return None;
        }

        // PROCESS_QUERY_LIMITED_INFORMATION is enough for
        // QueryFullProcessImageNameW and is grantable even for processes at a
        // different integrity level.
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;

        let mut buf = [0u16; MAX_PATH as usize];
        // `len` is in/out: the buffer capacity going in, the written length out.
        let mut len: u32 = MAX_PATH;
        let path = PWSTR(buf.as_mut_ptr());
        let query = QueryFullProcessImageNameW(handle, PROCESS_NAME_FORMAT(0), path, &raw mut len);
        let _ = CloseHandle(handle);
        query.ok()?;

        let full_path = String::from_utf16_lossy(&buf[..len as usize]);
        if full_path.is_empty() {
            return None;
        }
        let name = full_path
            .rsplit(['\\', '/'])
            .next()
            .filter(|s| !s.is_empty())
            .map(str::to_owned);

        Some(FrontmostApp {
            name,
            bundle_id: Some(full_path),
        })
    }
}
