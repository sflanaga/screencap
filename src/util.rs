use std::{
    borrow::{Borrow, BorrowMut},
    collections::BinaryHeap,
    fs::File,
    path::PathBuf,
    time::{Duration, Instant},
};

use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Local};
use scrap::{Capturer, Display, Frame};
use std::io::ErrorKind::WouldBlock;

use crate::CliCfg;

pub fn now_str() -> String {
    let dt: DateTime<Local> = Local::now();
    dt.format("%Y-%m-%d %H:%M:%S").to_string()
}

pub fn create(display_num: &Option<usize>) -> Result<Option<Capturer>> {
    let display = if let Some(dn) = display_num {
        let mut v = Display::all()?;
        if v.len() < *dn {
            v.swap_remove(*dn) // O(1) way of taking ownership of a single vec item
        } else {
            return Err(anyhow!("cannot find display for index {}", dn));
        }
    } else {
        Display::primary().with_context(|| format!("Cannot find primary display due"))?
    };
    let capturer = Capturer::new(display)?;
    eprintln!("[{}]  good create", now_str());
    Ok(Some(capturer))
}

#[derive(Default)]
pub struct DiffStat {
    pix_cnt_diff: usize,
    sum_diff: usize,
}
pub struct ScreenWatch {
    screen: Option<Capturer>,
    display_num: Option<usize>,
    last_buff: Option<Vec<u8>>,
    delta_buff: Vec<u8>,
    last_cap_time: Option<Instant>,
    last_spin_count: usize,
    diff: DiffStat,
}

impl ScreenWatch {
    pub fn new(display_num: &Option<usize>) -> Result<Self> {
        let cap = create(display_num)?;
        let (h, w) = (cap.as_ref().unwrap().height(), cap.as_ref().unwrap().width());
        let v = vec![0u8; h * w * 4];
        let s = ScreenWatch {
            screen: cap,
            display_num: display_num.clone(),
            last_buff: None,
            delta_buff: v,
            last_cap_time: None,
            last_spin_count: 0,
            diff: DiffStat::default(),
        };

        Ok(s)
    }

    fn make_sure_of_screen(&mut self) {
        loop {
            if self.screen.is_none() {
                let mut cc = match create(&self.display_num) {
                    Ok(c) => c,
                    Err(e) => {
                        eprintln!("[{}] error on setup capt: {} - retry in 1 sec", now_str(), e);
                        std::thread::sleep(Duration::from_secs(1));
                        continue;
                    }
                };
                self.screen.replace(cc.unwrap());
                return;
            }
        }
    }

    //
    // These next 2 methods are here to show an alternative split ownership
    // The one chosen does away with the "unwrap()"
    //
    pub fn cap_diff2(&mut self) -> Result<bool> {
        match (self.screen.is_some(), self.last_buff.is_some()) {
            // normal - happy path
            (true, true) => return self.cap_diff_inner(),
            // first screen comparison
            _ => return Ok(false),
        }
    }

    fn cap_diff_inner(&mut self) -> Result<bool> {
        // split the ownership into parts - makes things possible OR maybe just simpler?
        let (last_buff, screen, delta_buff) = (self.last_buff.as_mut().unwrap(), self.screen.as_mut().unwrap(), &mut self.delta_buff);

        let frame = Self::capture_retrier(screen)?;
        let mut cnt = 0usize;
        let mut sumdiff = 0usize;
        last_buff.iter().zip(&frame[..]).enumerate().for_each(|(i, (last, curr))| -> () {
            let (s, c) = absdiff(*last, *curr);
            delta_buff[i] = s;
            cnt += c as usize;
            sumdiff += s as usize;
        });
        self.diff.pix_cnt_diff = cnt;
        self.diff.sum_diff = sumdiff;
        // println!("diff cnt: {}  diff sum: {}", cnt, sumdiff);
        if cnt > 1_000_000 || sumdiff > 5_000_000 {
            last_buff.clone_from_slice(&frame[..]);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn cap_diff(&mut self, cnt_diff: usize, sum_diff: usize) -> Result<bool> {
        match (self.screen.as_mut(), self.last_buff.as_mut()) {
            // normal - happy path
            (Some(mut screen), Some(mut last_buff)) => {
                return Self::cap_diff_split(&mut screen, &mut last_buff, &mut self.delta_buff, &mut self.diff, cnt_diff, sum_diff)
            }
            // first screen comparison
            (Some(screen), None) => {
                self.last_buff = Some(Self::capture_retrier(screen)?);
                return Ok(false);
            }
            // we lost the capterer-ererr
            (None, Some(mut last_buff)) => {
                self.last_buff = None; // new screen - need to new buff to start comparisons with?
                self.screen = create(&self.display_num)?;
                // or do we? - not sure

                return Ok(false);
            }
            _ => return Ok(false),
        }
    }

    pub fn last_cap(&self) -> &Option<Vec<u8>> {
        &self.last_buff
    }

    pub fn capture_retrier(cap: &mut Capturer) -> Result<Vec<u8>> {
        let mut count = 0;
        let res = loop {
            {
                match cap.frame() {
                    Ok(buffer) => {
                        // so yes, this is a way to prevent this allocation...
                        // but I'm lazy
                        let mut v = vec![0u8; buffer.len()];
                        //println!("copy buff");
                        v.clone_from_slice(&buffer[..]);
                        break Ok(v);
                    }
                    Err(e) => {
                        if e.kind() == WouldBlock {
                            // Keep spinning.
                            std::thread::sleep(Duration::from_millis(10));
                            count += 1;
                            continue;
                        } else {
                            break Err(anyhow!("Error other than would block in capture: {}", e));
                        }
                    }
                }
            }
        };
        res
    }

    // This routines assumes the split borrow mutable is done by a caller
    // a tuple assignment seems work as well, but this one removes the need to unwrap
    // meaning it is safer/clear maybe?
    fn cap_diff_split(cap: &mut Capturer, last_buff: &mut Vec<u8>, delta_buff: &mut Vec<u8>, diff: &mut DiffStat, cnt_diff: usize, sum_diff: usize) -> Result<bool> {
        let frame = Self::capture_retrier(cap)?;
        let mut cnt = 0usize;
        let mut sumdiff = 0usize;
        last_buff.iter().zip(&frame[..]).enumerate().for_each(|(i, (last, curr))| -> () {
            let (s, c) = absdiff(*last, *curr);
            delta_buff[i] = s;
            cnt += c as usize;
            sumdiff += s as usize;
        });

        diff.pix_cnt_diff = cnt;
        diff.sum_diff = sumdiff;

        last_buff.clone_from_slice(&frame[..]);
        println!("diff cnt: {}  diff sum: {}", cnt, sumdiff);
        if cnt_diff != 0 && cnt > cnt_diff || sum_diff != 0 && sumdiff > 500_000 {
            Ok(true)
        } else {
            Ok(false)
        }
    }
    pub fn write_delta_buff_png(&self, path: &PathBuf) -> Result<()> {
        return self.write_buff(&self.delta_buff, path);
    }
    pub fn write_last_buff_png(&self, path: &PathBuf) -> Result<()> {
        return self.write_buff(self.last_buff.as_ref().unwrap(), path);
    }

    fn write_buff(&self, buff: &Vec<u8>, path: &PathBuf) -> Result<()> {
        let h = self.screen.as_ref().unwrap().height();
        let w = self.screen.as_ref().unwrap().width();
        let mut bitflipped = Vec::with_capacity(h * w * 4);
        let stride = buff.len() / h;

        for y in 0..h {
            for x in 0..w {
                let i = stride * y + 4 * x;
                bitflipped.extend_from_slice(&[buff[i + 2], buff[i + 1], buff[i], 255]);
            }
        }
        let mut file = File::create(&path)?; //.with_context(||format!("Cannot create file {}", path.display()))?;
        repng::encode(&mut file, w as u32, h as u32, &bitflipped)?; //.context("failure to encode bits from screen grab as png")?;

        Ok(())
    }

    /*

            split_own_diff(&mut cap)

            let ONE_FRAME: Duration = Duration::new(1, 0) / 60;
            self.last_spin_count = 0;
            loop {
                self.make_sure_of_screen();

                match (self.screen.is_some(), self.last_buff.is_some()) {
                    (true, true) => {
                        Self::diff_frame( self.screen.unwrap(), self.last_buff.unwrap());


                    },
                    _ => {}
                }

            }
        }
    */

    /*
        fn _diff_frame(last_buff: &mut Vec<u8>, frame: &Frame) -> bool {
                let mut cnt = 0;
                let mut sumdiff = 0;
                last_buff.iter().zip(&frame[..]).for_each(|(last, curr)| -> () {
                    let (s, c) = absdiff(*last, *curr);
                    cnt += c as usize;
                    sumdiff += s as usize;
                });
                println!("diff cnt: {}  diff sum: {}", cnt, sumdiff);
                if cnt > 1000 || sumdiff > 100000 {
                    last_buff.clone_from_slice(&frame[..]);
                    true
                } else {
                    false
                }
            // } else {
            //     let w = self.screen.as_ref().unwrap().width();
            //     let h = self.screen.as_ref().unwrap().height();
            //     self.last_buff = Some(vec![0u8; w * h * 4]);
            //     true
            // }
        }
    */
}

fn absdiff(a: u8, b: u8) -> (u8, u8) {
    if a == b {
        (0u8, 0)
    } else if a > b {
        (a - b, 1)
    } else {
        (b - a, 1)
    }
}
