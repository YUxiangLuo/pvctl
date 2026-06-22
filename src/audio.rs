use std::{collections::HashMap, process::Command};

use anyhow::{Context as _, Result, anyhow, bail};
use serde::Deserialize;

const PA_VOLUME_NORM: f32 = 65_536.0;

#[derive(Clone, Debug, PartialEq)]
pub struct AudioState {
    pub sinks: Vec<AudioSink>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AudioSink {
    pub index: u32,
    pub name: String,
    pub description: String,
    pub muted: bool,
    pub volume_percent: f32,
    pub is_default: bool,
}

#[derive(Debug, Deserialize)]
struct PactlInfo {
    default_sink_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PactlSink {
    index: u32,
    name: String,
    description: Option<String>,
    mute: bool,
    volume: HashMap<String, PactlVolume>,
}

#[derive(Debug, Deserialize)]
struct PactlVolume {
    value: u32,
}

pub fn load_state() -> Result<AudioState> {
    let default_sink_name = load_default_sink_name()?;
    let mut sinks = load_sinks()?;

    for sink in &mut sinks {
        sink.is_default = default_sink_name
            .as_ref()
            .is_some_and(|default| default == &sink.name);
    }

    Ok(AudioState { sinks })
}

pub fn set_sink_volume(sink_name: &str, percent: u32) -> Result<()> {
    let volume = format!("{}%", percent.min(100));
    run_pactl_status(&["set-sink-volume", sink_name, &volume])
        .with_context(|| format!("failed to set volume for {sink_name}"))
}

pub fn set_default_sink(sink_name: &str) -> Result<()> {
    run_pactl_status(&["set-default-sink", sink_name])
        .with_context(|| format!("failed to set default sink to {sink_name}"))
}

fn load_default_sink_name() -> Result<Option<String>> {
    let output = run_pactl_output(&["-f", "json", "info"])?;
    let info: PactlInfo =
        serde_json::from_str(&output).context("failed to parse `pactl info` JSON")?;
    Ok(info.default_sink_name)
}

fn load_sinks() -> Result<Vec<AudioSink>> {
    let output = run_pactl_output(&["-f", "json", "list", "sinks"])?;
    let sinks: Vec<PactlSink> =
        serde_json::from_str(&output).context("failed to parse `pactl list sinks` JSON")?;

    Ok(sinks
        .into_iter()
        .map(|sink| AudioSink {
            index: sink.index,
            name: sink.name,
            description: sink
                .description
                .unwrap_or_else(|| "Unknown sink".to_string()),
            muted: sink.mute,
            volume_percent: volume_percent(&sink.volume),
            is_default: false,
        })
        .collect())
}

fn volume_percent(channels: &HashMap<String, PactlVolume>) -> f32 {
    if channels.is_empty() {
        return 0.0;
    }

    let sum = channels
        .values()
        .map(|volume| volume.value as f32 / PA_VOLUME_NORM * 100.0)
        .sum::<f32>();

    (sum / channels.len() as f32).clamp(0.0, 100.0)
}

fn run_pactl_output(args: &[&str]) -> Result<String> {
    let output = Command::new("pactl")
        .args(args)
        .output()
        .with_context(|| format!("failed to execute pactl {}", args.join(" ")))?;

    if !output.status.success() {
        bail!(pactl_error(args, &output));
    }

    String::from_utf8(output.stdout).context("pactl output was not valid UTF-8")
}

fn run_pactl_status(args: &[&str]) -> Result<()> {
    let output = Command::new("pactl")
        .args(args)
        .output()
        .with_context(|| format!("failed to execute pactl {}", args.join(" ")))?;

    if !output.status.success() {
        bail!(pactl_error(args, &output));
    }

    Ok(())
}

fn pactl_error(args: &[&str], output: &std::process::Output) -> anyhow::Error {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    anyhow!(
        "pactl {} exited with {}\nstderr: {}\nstdout: {}",
        args.join(" "),
        output.status,
        stderr.trim(),
        stdout.trim()
    )
}
