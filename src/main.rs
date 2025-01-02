use std::env;
use std::ffi::c_void;
use std::io::{BufRead, Error as IoError, Read};
use std::mem::size_of;
use std::process;
use std::ptr::null_mut;
use std::slice;

use windows::core::{PCWSTR, PWSTR, w};
use windows::Win32::Foundation::{ERROR_NO_MORE_ITEMS, HANDLE, HWND, NO_ERROR};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::Win32::NetworkManagement::WNet::{
    CONNECT_INTERACTIVE, CONNECT_PROMPT, CONNECT_TEMPORARY,  NETRESOURCEW, NET_RESOURCE_SCOPE,
    RESOURCETYPE_DISK, RESOURCE_CONNECTED, WNET_OPEN_ENUM_USAGE, WNetAddConnection2W, WNetCloseEnum,
    WNetEnumResourceW, WNetOpenEnumW,
};


fn wcstr_to_string(ptr: *const u16) -> String {
    let mut moving_ptr = ptr;
    let mut utf16_buf = Vec::new();
    while unsafe { *moving_ptr } != 0x0000 {
        utf16_buf.push(unsafe { *moving_ptr });
        moving_ptr = moving_ptr.wrapping_add(1);
    }
    String::from_utf16(&utf16_buf).unwrap()
}

fn str_to_wcstring(s: &str) -> Vec<u16> {
    let mut ret = Vec::with_capacity(s.len() + 1);
    for w in s.encode_utf16() {
        ret.push(w);
    }
    ret.push(0);
    ret
}


fn is_connection_already_open(path: &str) -> bool {
    let path_lower = path.to_lowercase();

    // check if a connection exists already
    let mut enum_handle = HANDLE(null_mut());
    let result = unsafe {
        WNetOpenEnumW(
            RESOURCE_CONNECTED,
            RESOURCETYPE_DISK,
            WNET_OPEN_ENUM_USAGE(0),
            None,
            &mut enum_handle,
        )
    };
    if result != NO_ERROR {
        eprintln!("failed to enumerate existing connections! {}", IoError::from_raw_os_error(result.0 as i32));
        eprintln!("assuming connection is not yet open...");
        return false;
    }

    let mut buffer = vec![0u8; 16*1024];
    let mut found = false;
    loop {
        let mut count = -1i32 as u32;
        let mut buf_size: u32 = buffer.len().try_into().unwrap();
        let result = unsafe {
            WNetEnumResourceW(
                enum_handle,
                &mut count,
                buffer.as_mut_ptr() as *mut c_void,
                &mut buf_size,
            )
        };

        if result == ERROR_NO_MORE_ITEMS {
            break;
        } else if result != NO_ERROR {
            eprintln!("failed to obtain more connection enumeration results! {}", IoError::from_raw_os_error(result.0 as i32));
            eprintln!("assuming connection is not yet open...");
            let _ = unsafe {
                WNetCloseEnum(enum_handle)
            };
            return false;
        }

        // read memory as struct
        let struct_size = size_of::<NETRESOURCEW>();
        let structs_read: usize = count.try_into().unwrap();
        let mut structs = vec![NETRESOURCEW::default(); structs_read];
        unsafe {
            let structs_slice = slice::from_raw_parts_mut(
                structs.as_mut_ptr() as *mut u8,
                struct_size * structs_read,
            );
            buffer.as_slice().read_exact(structs_slice).unwrap();
        }

        // extract path
        for st in structs {
            if st.lpRemoteName.0 == null_mut() {
                continue;
            }
            let remote_path_lower = wcstr_to_string(st.lpRemoteName.0)
                .to_lowercase();
            eprintln!("testing against path: {:?}", remote_path_lower);
            if remote_path_lower == path_lower {
                // we know this path!
                found = true;
                break;
            }
        }
    }

    let result = unsafe {
        WNetCloseEnum(HANDLE(enum_handle.0))
    };
    if result != NO_ERROR {
        eprintln!("failed to close existing connection enumeration! {}", IoError::from_raw_os_error(result.0 as i32));
    }

    found
}

fn connect_to_share(path: &str, username: &str) -> bool {
    let mut path_windows = str_to_wcstring(path);
    let path_pwstr = PWSTR(path_windows.as_mut_ptr());

    let username_windows = str_to_wcstring(username);
    let username_pcwstr = PCWSTR(username_windows.as_ptr());

    let net_resource = NETRESOURCEW {
        dwType: RESOURCETYPE_DISK,
        lpLocalName: PWSTR(null_mut()),
        lpRemoteName: path_pwstr,
        lpProvider: PWSTR(null_mut()),

        dwDisplayType: 0,
        dwUsage: 0,
        dwScope: NET_RESOURCE_SCOPE(0),
        lpComment: PWSTR(null_mut()),
    };

    let result = unsafe {
        WNetAddConnection2W(
            &net_resource,
            None,
            username_pcwstr,
            CONNECT_INTERACTIVE | CONNECT_PROMPT | CONNECT_TEMPORARY,
        )
    };
    if result != NO_ERROR {
        eprintln!("failed to connect! {}", IoError::from_raw_os_error(result.0 as i32));
        return false;
    }
    eprintln!("connected!");
    true
}

fn open_path(path: &str) -> bool {
    let path_windows = str_to_wcstring(path);

    let result = unsafe {
        ShellExecuteW(
            HWND(null_mut()),
            w!("open"),
            PCWSTR(path_windows.as_ptr()),
            None,
            None,
            SW_SHOWNORMAL,
        )
    };
    let result_int = result.0 as usize;
    if result_int <= 32 {
        eprintln!("failed to open share! {}", IoError::from_raw_os_error(result_int as i32));
        return false;
    }
    eprintln!("launched!");
    true
}


fn inner_main() -> i32 {
    let args: Vec<String> = env::args().collect();
    let program_name: &str = match args.get(0) {
        Some(pn) => pn,
        None => "open-share",
    };
    if args.len() != 3 {
        eprintln!("Usage: {} PATH USERNAME", program_name);
        return 1;
    }

    let path = args.get(1).unwrap();
    let username = args.get(2).unwrap();

    if !is_connection_already_open(&path) {
        if !connect_to_share(path, username) {
            return 1;
        }
    }

    eprintln!("launching...");
    let result = open_path(path);

    if result { 0 } else { 1 }
}

fn main() {
    let exit_code = inner_main();

    if exit_code != 0 {
        eprintln!("exiting with {}", exit_code);
        eprintln!("press Enter to exit (oddly enough)");

        let si = std::io::stdin();
        let mut sil = si.lock();
        let mut buf = String::new();
        sil.read_line(&mut buf)
            .expect("failed to read line");
    }

    process::exit(exit_code);
}
