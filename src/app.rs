use core::cell::{Cell, RefCell};
use alloc::vec::Vec;
use alloc::collections::vec_deque::VecDeque;
use embassy_time::Instant;
use core::sync::atomic::Ordering;

#[derive(Clone, Copy, PartialEq)]
pub enum Page {
    Welcome,
    Dashboard,
    Amplitude,
    Phase,
    Heatmap,
    Info,
}

impl Page {
    pub fn next(&self) -> Self {
        match self {
            Page::Welcome => Page::Dashboard,
            Page::Dashboard => Page::Amplitude,
            Page::Amplitude => Page::Phase,
            Page::Phase => Page::Heatmap,
            Page::Heatmap => Page::Info,
            Page::Info => Page::Dashboard,
            _ => Page::Welcome,
        }
    }
}

pub struct AppState {
    pub current_page: Page,
    pub packet_count: Cell<u32>,
    pub current_rssi: Cell<i8>,
    pub start_time: Option<Instant>,
    pub csi_i: RefCell<Vec<(f64, f64)>>,
    pub csi_q: RefCell<Vec<(f64, f64)>>,
    pub csi_amp: RefCell<Vec<(f64, f64)>>,
    pub csi_phase: RefCell<Vec<(f64, f64)>>,
    pub heatmap_history: RefCell<VecDeque<Vec<f64>>>,
    pub csi_file: RefCell<Option<()>>, 
}

impl AppState {
    pub fn new() -> Self {
        // PRE-ALLOCATE arrays so we never touch the heap during runtime
        let mut i_vec = Vec::with_capacity(64);
        let mut q_vec = Vec::with_capacity(64);
        let mut amp_vec = Vec::with_capacity(64);
        let mut phs_vec = Vec::with_capacity(64);
        
        for i in 0..64 {
            let x = i as f64;
            i_vec.push((x, 0.0));
            q_vec.push((x, 0.0));
            amp_vec.push((x, 0.0));
            phs_vec.push((x, 0.0));
        }

        Self {
            current_page: Page::Welcome,
            packet_count: Cell::new(0),
            current_rssi: Cell::new(0),
            start_time: None,
            csi_i: RefCell::new(i_vec),
            csi_q: RefCell::new(q_vec),
            csi_amp: RefCell::new(amp_vec),
            csi_phase: RefCell::new(phs_vec),
            heatmap_history: RefCell::new(VecDeque::with_capacity(81)),
            csi_file: RefCell::new(None),
        }
    }

    pub fn update_data(&self) {
        let mut i_data = self.csi_i.borrow_mut();
        let mut q_data = self.csi_q.borrow_mut();
        let mut amp_data = self.csi_amp.borrow_mut();
        let mut phs_data = self.csi_phase.borrow_mut();
        let mut history = self.heatmap_history.borrow_mut();
        
        // Recycle heatmap vectors
        let mut raw_amps = if history.len() >= 80 {
            history.pop_back().unwrap() 
        } else {
            alloc::vec![0.0; 64] 
        };

        for i in 0..64 {
            // FPU Hardware Math: Cast to f32 so the chip does the math instantly
            let im = crate::CSI_RAW_Q[i].load(Ordering::Relaxed) as i8 as f32;
            let re = crate::CSI_RAW_I[i].load(Ordering::Relaxed) as i8 as f32;
            
            let amp = libm::sqrtf(im * im + re * re);
            let phase = libm::atan2f(im, re);

            // Update in place
            i_data[i].1 = im as f64;
            q_data[i].1 = re as f64;
            phs_data[i].1 = phase as f64;
            raw_amps[i] = amp as f64;

            // EMA Smoothing: 70% old data, 30% new data. Removes the "choppiness".
            let old_amp = amp_data[i].1;
            amp_data[i].1 = (old_amp * 0.7) + ((amp as f64) * 0.3); 
        }

        self.current_rssi.set(crate::CSI_RSSI.load(Ordering::Relaxed) as i8);
        history.push_front(raw_amps);
        self.packet_count.set(self.packet_count.get() + 1);
    }
}