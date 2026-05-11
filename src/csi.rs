use core::sync::atomic::{AtomicU8, AtomicI32, AtomicBool, AtomicUsize, Ordering};
use embassy_time::{with_timeout, Duration, Timer, Instant};
use esp_csi_rs::CSINodeClient;
use micromath::F32Ext;
use core::f32::consts::PI;
use esp_println::println;

const ATOMIC_U8_INIT: AtomicU8 = AtomicU8::new(0);
const ATOMIC_I32_INIT: AtomicI32 = AtomicI32::new(0);

pub static CSI_AMPLITUDES: [AtomicU8; 64] = [ATOMIC_U8_INIT; 64];
pub static CSI_PHASES: [AtomicI32; 64] = [ATOMIC_I32_INIT; 64]; 
pub static CURRENT_RSSI: AtomicI32 = AtomicI32::new(-100);

pub static SELECTED_MODE: AtomicU8 = AtomicU8::new(0); 
pub static RECORDING_STATE: AtomicU8 = AtomicU8::new(0);

pub static NEW_CSV_READY: AtomicBool = AtomicBool::new(false);
pub static CSV_LEN: AtomicUsize = AtomicUsize::new(0);
pub static mut CSV_BUFFER: [u8; 4096] = [0; 4096]; 

// NEW: Packet counter
pub static PACKET_COUNT: AtomicUsize = AtomicUsize::new(0);

pub async fn node_task(client: &mut CSINodeClient, _mode: u8) {
    loop {
        let state = RECORDING_STATE.load(Ordering::Relaxed);
        if state == 1 {
            println!(">>> PIPELINE HALTED: Waiting for Save/Delete decision...");
            break; 
        }

        match with_timeout(Duration::from_millis(1500), client.get_csi_data()).await {
            Ok(csi_data) => {
                let raw = csi_data.csi_data();
                
                // Increment packet count on successful receive
                PACKET_COUNT.fetch_add(1, Ordering::Relaxed);
                
                let rssi = csi_data.rssi();
                CURRENT_RSSI.store(rssi as i32, Ordering::Relaxed);
                
                let num_subcarriers = core::cmp::min(raw.len() / 2, 64);
                
                let mut prev_phase = 0.0;
                let mut unwrap_offset = 0.0;

                for i in 0..num_subcarriers {
                    let imaginary = raw[i * 2] as i8 as f32;
                    let real = raw[i * 2 + 1] as i8 as f32;
                    
                    // Amplitude
                    let amplitude = (imaginary * imaginary + real * real).sqrt() as u8;
                    CSI_AMPLITUDES[i].store(amplitude, Ordering::Relaxed);

                    // Phase Calculation & Unwrapping
                    let mut phase = imaginary.atan2(real);
                    
                    if i > 0 {
                        let diff = phase - prev_phase;
                        if diff > PI {
                            unwrap_offset -= 2.0 * PI;
                        } else if diff < -PI {
                            unwrap_offset += 2.0 * PI;
                        }
                    }
                    prev_phase = phase;
                    phase += unwrap_offset;

                    // Multiply by 1000 so we can safely store the float across threads as an integer
                    CSI_PHASES[i].store((phase * 1000.0) as i32, Ordering::Relaxed);
                }
                
                let mac = csi_data.mac();   
                let timestamp = Instant::now().as_millis();

                let mut row = alloc::format!("{},{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X},{},0,0,0,{},\"[", 
                    timestamp, mac[0], mac[1], mac[2], mac[3], mac[4], mac[5], rssi, raw.len());
                
                for (idx, byte) in raw.iter().enumerate() {
                    if idx > 0 { row.push_str(", "); }
                    row.push_str(&alloc::format!("{}", *byte as i8));
                }
                row.push_str("]\"\n");

                if !NEW_CSV_READY.load(Ordering::Acquire) {
                    unsafe {
                        let bytes = row.as_bytes();
                        let len = core::cmp::min(bytes.len(), 4096);
                        CSV_BUFFER[..len].copy_from_slice(&bytes[..len]);
                        CSV_LEN.store(len, Ordering::Release);
                    }
                    NEW_CSV_READY.store(true, Ordering::Release);
                }
            }
            Err(_) => {
                for i in 0..64 { 
                    CSI_AMPLITUDES[i].store(0, Ordering::Relaxed); 
                    CSI_PHASES[i].store(0, Ordering::Relaxed);
                }
                CURRENT_RSSI.store(-100, Ordering::Relaxed);
            }
        }
    }

    loop {
        let final_decision = RECORDING_STATE.load(Ordering::Relaxed);
        if final_decision == 2 || final_decision == 3 { break; }
        Timer::after(Duration::from_millis(100)).await;
    }
}
