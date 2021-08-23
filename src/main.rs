use std::{io::Write, path::PathBuf, time::Duration};

use anyhow::{anyhow, Context};
use chrono::{DateTime, Local};
use scrap::{Capturer, Display};
use structopt::StructOpt;

mod util;
use util::*;

#[derive(StructOpt, Debug, Clone)]
#[structopt(
global_settings(& [structopt::clap::AppSettings::ColoredHelp, structopt::clap::AppSettings::VersionlessSubcommands, structopt::clap::AppSettings::DeriveDisplayOrder]),
//raw(setting = "structopt::clap::AppSettings::DeriveDisplayOrder"),
author, about
)]
pub struct CliCfg {
    #[structopt(short = "d", long = "use_display")]
    /// Use a display for capture - "primary is default" - starts at 0
    pub use_display: Option<usize>,

    #[structopt(short = "v", parse(from_occurrences))]
    /// Verbosity - use more than one v for greater detail
    pub verbose: usize,
}

fn main() {
    let cfg = CliCfg::from_args();

    let start_dt: DateTime<Local> = Local::now();
    ctrlc::set_handler(move || {
        let dur = Local::now() - start_dt;
        println!("\n\n Exiting at {} ran for {:?}", &now_str(), dur);
        std::process::exit(0);
    })
    .expect("Error setting Ctrl-C handler");

    let mut sw = match util::ScreenWatch::new(&cfg.use_display) {
        Err(e) => {
            eprintln!("Cannot setup screen watch, reason: {}", &e);
            return;
        },
        Ok(sw) => sw,
    };

    loop {
        std::thread::sleep(Duration::from_secs(1));
        match sw.cap_diff(10000, 0) {
            Err(e) => eprintln!("capture diff failed: {}", e),
            Ok(diff) => {
                if diff {

                    print!(".");
                    std::io::stdout().flush().unwrap();
                    sw.write_delta_buff_png(&PathBuf::from("testing.png"));
                    print!(".");
                    std::io::stdout().flush().unwrap();

                } else {
                    //println!("NO difference of significance");
                }
            }
        }
    }
        
    


    println!("Hello, world!");
}


