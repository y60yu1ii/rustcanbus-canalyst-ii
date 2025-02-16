use libloading::Library;
use std::{
    sync::{Arc, atomic::{AtomicBool, Ordering}},
    thread,
    time::Duration,
};
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::terminal::{enable_raw_mode, disable_raw_mode};

#[repr(C)]
#[derive(Debug, Default)]
struct VciCanObj {
    id: u32,
    time_stamp: u32,
    time_flag: u8,
    send_type: u8,
    remote_flag: u8,
    extern_flag: u8,
    data_len: u8,
    data: [u8; 8],
    reserved: [u8; 3],
}

#[repr(C)]
#[derive(Debug, Default)]
struct VciInitConfig {
    acc_code: u32,
    acc_mask: u32,
    reserved: u32,
    filter: u8,
    timing0: u8,
    timing1: u8,
    mode: u8,
}

struct CanLibrary {
    _lib: Arc<Library>,
    vci_open_device: unsafe extern "stdcall" fn(u32, u32, u32) -> i32,
    vci_close_device: unsafe extern "stdcall" fn(u32, u32) -> i32,
    vci_init_can: unsafe extern "stdcall" fn(u32, u32, u32, *const VciInitConfig) -> i32,
    vci_start_can: unsafe extern "stdcall" fn(u32, u32, u32) -> i32,
    vci_transmit: unsafe extern "stdcall" fn(u32, u32, u32, *const VciCanObj, u32) -> i32,
    vci_receive: unsafe extern "stdcall" fn(u32, u32, u32, *mut VciCanObj, u32, i32) -> i32,
}

impl CanLibrary {
    fn new(dll_name: &str) -> Arc<Self> {
        let lib = Arc::new(unsafe { Library::new(dll_name) }.expect("DLL load failed"));

        unsafe {
            Arc::new(Self {
                _lib: lib.clone(),
                vci_open_device: *lib.get(b"VCI_OpenDevice").expect("Failed to get VCI_OpenDevice"),
                vci_close_device: *lib.get(b"VCI_CloseDevice").expect("Failed to get VCI_CloseDevice"),
                vci_init_can: *lib.get(b"VCI_InitCAN").expect("Failed to get VCI_InitCAN"),
                vci_start_can: *lib.get(b"VCI_StartCAN").expect("Failed to get VCI_StartCAN"),
                vci_transmit: *lib.get(b"VCI_Transmit").expect("Failed to get VCI_Transmit"),
                vci_receive: *lib.get(b"VCI_Receive").expect("Failed to get VCI_Receive"),
            })
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dll = CanLibrary::new("ControlCAN.dll");

    let dev_type = 4;
    let dev_index = 0;
    let can1 = 0;
    let can2 = 1;
    let reserved = 0;

    if unsafe { (dll.vci_open_device)(dev_type, dev_index, reserved) } != 1 {
        println!("Failed to open device");
        return Ok(());
    }
    println!("Device opened successfully");

    let config = VciInitConfig {
        acc_code: 0,
        acc_mask: 0xFFFFFFFF,
        reserved: 0,
        filter: 1,
        timing0: 0x01,
        timing1: 0x1C,
        mode: 0,
    };

    if unsafe { (dll.vci_init_can)(dev_type, dev_index, can1, &config) } != 1 {
        println!("Failed to initialize CAN1");
        return Ok(());
    }
    if unsafe { (dll.vci_init_can)(dev_type, dev_index, can2, &config) } != 1 {
        println!("Failed to initialize CAN2");
        return Ok(());
    }
    println!("CAN1 & CAN2 initialized successfully (250kbps)");

    if unsafe { (dll.vci_start_can)(dev_type, dev_index, can1) } != 1 {
        println!("Failed to start CAN1");
        return Ok(());
    }
    if unsafe { (dll.vci_start_can)(dev_type, dev_index, can2) } != 1 {
        println!("Failed to start CAN2");
        return Ok(());
    }
    println!("CAN1 & CAN2 started. Ready for transmission and reception");

    let running = Arc::new(AtomicBool::new(true));

    let running_clone = Arc::clone(&running);
    let keyboard_thread = thread::spawn(move || {
        enable_raw_mode().expect("Failed to enable raw mode");
        println!("Press 'Ctrl + X' to exit...");

        while running_clone.load(Ordering::SeqCst) {
            if event::poll(Duration::from_millis(100)).unwrap() {
                if let Event::Key(key) = event::read().unwrap() {
                    if key.code == KeyCode::Char('x') && key.modifiers.contains(KeyModifiers::CONTROL) {
                        println!("Ctrl + X detected, closing...");
                        running_clone.store(false, Ordering::SeqCst);
                        break;
                    }
                }
            }
        }

        disable_raw_mode().expect("Failed to disable raw mode");
    });

    let running_clone1 = Arc::clone(&running);
    let dll_clone1 = Arc::clone(&dll);

    let receive_thread = thread::spawn(move || {
        unsafe {
            while running_clone1.load(Ordering::SeqCst) {
                let mut recv_obj: VciCanObj = VciCanObj::default();
                let received_frames = (dll_clone1.vci_receive)(dev_type, dev_index, can1, &mut recv_obj, 1, 500);

                if received_frames > 0 {
                    println!("CAN1 received: ID=0x{:X}, Data={:?}", recv_obj.id, &recv_obj.data[..recv_obj.data_len as usize]);
                }
                thread::sleep(Duration::from_millis(5));
            }
        }
    });

    let dll_clone3 = Arc::clone(&dll);
    let transmit_thread = thread::spawn(move || {
        unsafe {
            for data in 1..=255 {
                let can_obj = VciCanObj {
                    id: 0x1,
                    data_len: 1,
                    data: [data, 0, 0, 0, 0, 0, 0, 0],
                    ..Default::default()
                };

                let sent_frames = (dll_clone3.vci_transmit)(dev_type, dev_index, can1, &can_obj, 1);
                if sent_frames > 0 {
                    println!("CAN1 sent: {}", data);
                }

                thread::sleep(Duration::from_millis(10));
            }
        }
    });

    transmit_thread.join().unwrap();
    receive_thread.join().unwrap();
    keyboard_thread.join().unwrap();

    unsafe { (dll.vci_close_device)(dev_type, dev_index) };
    println!("Device closed");

    Ok(())
}
