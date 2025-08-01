use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::config::Config;
use crate::printtr;

use tr::tr;

#[derive(Debug, Clone)]
pub struct PackageTiming {
    pub package: String,
    pub download_time: Option<Duration>,
    pub build_time: Option<Duration>,
    pub install_time: Option<Duration>,
    pub total_time: Duration,
}

#[derive(Debug)]
pub struct TimingContext {
    pub start_time: Instant,
    pub package_timings: HashMap<String, PackageTiming>,
    current_package: Option<String>,
    current_phase_start: Option<Instant>,
}

impl TimingContext {
    pub fn new() -> Self {
        Self {
            start_time: Instant::now(),
            package_timings: HashMap::new(),
            current_package: None,
            current_phase_start: None,
        }
    }

    pub fn start_package(&mut self, package: &str) {
        self.current_package = Some(package.to_string());
        self.package_timings.insert(
            package.to_string(),
            PackageTiming {
                package: package.to_string(),
                download_time: None,
                build_time: None,
                install_time: None,
                total_time: Duration::ZERO,
            },
        );
    }

    pub fn start_phase(&mut self) {
        self.current_phase_start = Some(Instant::now());
    }

    pub fn end_phase(&mut self, phase: &str) -> Option<Duration> {
        if let (Some(package), Some(start)) =
            (&self.current_package, self.current_phase_start.take())
        {
            let duration = start.elapsed();
            if let Some(timing) = self.package_timings.get_mut(package) {
                match phase {
                    "download" => timing.download_time = Some(duration),
                    "build" => timing.build_time = Some(duration),
                    "install" => timing.install_time = Some(duration),
                    _ => {}
                }
                return Some(duration);
            }
        }
        None
    }

    pub fn end_package(&mut self) -> Option<Duration> {
        if let Some(package) = self.current_package.take() {
            if let Some(timing) = self.package_timings.get_mut(&package) {
                timing.total_time = timing.download_time.unwrap_or(Duration::ZERO)
                    + timing.build_time.unwrap_or(Duration::ZERO)
                    + timing.install_time.unwrap_or(Duration::ZERO);
                return Some(timing.total_time);
            }
        }
        None
    }

    pub fn print_summary(&self, config: &Config) {
        if self.package_timings.is_empty() {
            return;
        }

        println!();
        printtr!("Package Build Times");

        let mut timings: Vec<_> = self.package_timings.values().collect();
        timings.sort_by(|a, b| b.total_time.cmp(&a.total_time));

        for timing in timings {
            let time_str = format_duration(timing.total_time);
            println!(
                "  {}: {}",
                config.color.bold.paint(&timing.package),
                config.color.stats_value.paint(&time_str)
            );
        }

        let total_time = self.start_time.elapsed();
        println!();
        println!(
            "{}: {}",
            config.color.bold.paint(tr!("Total build time")),
            config.color.stats_value.paint(format_duration(total_time))
        );
    }

    pub fn print_detailed(&self, config: &Config) {
        if self.package_timings.is_empty() {
            return;
        }

        println!();
        printtr!("Package Build Times (Detailed)");

        let mut timings: Vec<_> = self.package_timings.values().collect();
        timings.sort_by(|a, b| b.total_time.cmp(&a.total_time));

        for timing in timings {
            println!();
            println!("  {}", config.color.bold.paint(&timing.package));

            if let Some(d) = timing.download_time {
                println!(
                    "    {}: {}",
                    tr!("Download"),
                    config.color.stats_value.paint(format_duration(d))
                );
            }
            if let Some(d) = timing.build_time {
                println!(
                    "    {}: {}",
                    tr!("Build"),
                    config.color.stats_value.paint(format_duration(d))
                );
            }
            if let Some(d) = timing.install_time {
                println!(
                    "    {}: {}",
                    tr!("Install"),
                    config.color.stats_value.paint(format_duration(d))
                );
            }
            println!(
                "    {}: {}",
                config.color.bold.paint(tr!("Total")),
                config
                    .color
                    .stats_value
                    .paint(format_duration(timing.total_time))
            );
        }

        let total_time = self.start_time.elapsed();
        println!();
        println!(
            "{}: {}",
            config.color.bold.paint(tr!("Total time")),
            config.color.stats_value.paint(format_duration(total_time))
        );
    }
}

fn format_duration(duration: Duration) -> String {
    let total_secs = duration.as_secs();
    let hours = total_secs / 3600;
    let mins = (total_secs % 3600) / 60;
    let secs = total_secs % 60;

    if hours > 0 {
        format!("{}h {}m {}s", hours, mins, secs)
    } else if mins > 0 {
        format!("{}m {}s", mins, secs)
    } else {
        format!("{}s", secs)
    }
}
