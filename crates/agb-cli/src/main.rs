//! `agb` — Agent Groovebox CLI. Prints one JSON document to stdout.
//! Exit codes: 0 success, 1 error response, 2 usage error.

use agb_assets::AssetKind;
use agb_cli::*;
use agb_protocol::SampleFormat;
use clap::{Parser, Subcommand, ValueEnum};
use std::io::Read;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "agb", version, about = "Agent-first headless groovebox (P0: deterministic offline WAV)")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Project lifecycle and inspection
    Project {
        #[command(subcommand)]
        cmd: ProjectCmd,
    },
    /// Apply JSON transactions
    Txn {
        #[command(subcommand)]
        cmd: TxnCmd,
    },
    /// Offline rendering
    Render {
        #[command(subcommand)]
        cmd: RenderCmd,
    },
    /// Logical transport position (used by latePolicy)
    Transport {
        #[command(subcommand)]
        cmd: TransportCmd,
    },
    /// Built-in preset catalog
    Preset {
        #[command(subcommand)]
        cmd: PresetCmd,
    },
    /// Synth engines
    Engine {
        #[command(subcommand)]
        cmd: EngineCmd,
    },
    /// JSON Schemas
    Schema {
        #[command(subcommand)]
        cmd: SchemaCmd,
    },
    /// Project assets and licenses
    Asset {
        #[command(subcommand)]
        cmd: AssetCmd,
    },
    /// `.agbsong` files: the interchange format (see FORMAT.md)
    Song {
        #[command(subcommand)]
        cmd: SongCmd,
    },
    /// Render a single note to a WAV (e.g. pad samples)
    Oneshot {
        #[arg(long)]
        engine: String,
        #[arg(long)]
        preset: Option<String>,
        /// Engine params as JSON object
        #[arg(long)]
        params: Option<String>,
        #[arg(long, default_value = "C3")]
        pitch: String,
        #[arg(long, default_value_t = 400)]
        len_ms: u32,
        #[arg(long, default_value_t = 1500)]
        tail_ms: u32,
        #[arg(long)]
        out: PathBuf,
    },
    /// AI performance: an LLM composes section by section; streams NDJSON events
    Perform {
        #[arg(long)]
        project: PathBuf,
        /// What to play (style, mood, instruments…)
        #[arg(long, default_value = "")]
        brief: String,
        /// Number of phrases (AI turns)
        #[arg(long, alias = "sections", default_value_t = 6)]
        phrases: u32,
        /// Default phrase length in bars (the AI may choose 2–16)
        #[arg(long, default_value_t = 8)]
        bars: u32,
        /// Pace rendering by the wall clock (live playback); late AI replies land on the next bar
        #[arg(long)]
        realtime: bool,
        #[arg(long, default_value_t = 0)]
        seed: u64,
        /// Model(s) in fallback order, comma separated
        #[arg(long, value_delimiter = ',')]
        model: Vec<String>,
        /// Gateway base URL (default $AGENTLLM_API_BASE or https://agentllm.linkyun.co)
        #[arg(long)]
        base_url: Option<String>,
        /// File holding the API key (default: $AGENTLLM_API_KEY)
        #[arg(long)]
        api_key_file: Option<PathBuf>,
        #[arg(long, default_value_t = 3)]
        max_retries: u32,
        /// Append sections to an existing performance
        #[arg(long = "continue")]
        continue_existing: bool,
        /// Use the built-in offline performer instead of an LLM
        #[arg(long)]
        mock: bool,
    },
}

#[derive(Subcommand)]
enum ProjectCmd {
    /// Create project.json and directory layout
    Init {
        #[arg(long)]
        project: PathBuf,
        #[arg(long)]
        name: Option<String>,
    },
    /// Print project state (+ derived timing info)
    Snapshot {
        #[arg(long)]
        project: PathBuf,
        /// Also print the score state in force at this tick
        #[arg(long)]
        at_tick: Option<u64>,
    },
}

#[derive(Subcommand)]
enum TxnCmd {
    /// Apply one transaction (creates the project if missing)
    Apply {
        #[arg(long)]
        project: PathBuf,
        #[arg(long, conflicts_with = "stdin", required_unless_present = "stdin")]
        file: Option<PathBuf>,
        #[arg(long)]
        stdin: bool,
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum Format {
    Pcm16,
    F32,
}

#[derive(Subcommand)]
enum RenderCmd {
    /// Render committed state to WAV + events/metrics/render JSON
    Offline {
        #[arg(long)]
        project: PathBuf,
        #[arg(long)]
        from_tick: Option<u64>,
        /// Default: one full polymeter cycle (LCM of layer lengths)
        #[arg(long)]
        to_tick: Option<u64>,
        #[arg(long)]
        seed: Option<u64>,
        #[arg(long)]
        stems: bool,
        /// Default 500
        #[arg(long)]
        tail_ms: Option<u32>,
        #[arg(long, value_enum, default_value = "pcm16")]
        format: Format,
        /// Output directory (mix.wav + sidecars). Default <project>/renders/latest
        #[arg(long, conflicts_with = "out")]
        out_dir: Option<PathBuf>,
        /// Mix file path; sidecar JSON files go next to it
        #[arg(long)]
        out: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum TransportCmd {
    Set {
        #[arg(long)]
        project: PathBuf,
        #[arg(long)]
        tick: u64,
    },
    Show {
        #[arg(long)]
        project: PathBuf,
    },
}

#[derive(Subcommand)]
enum PresetCmd {
    /// Search presets (all tags must match)
    Search {
        #[arg(long, value_delimiter = ',')]
        tags: Vec<String>,
        #[arg(long)]
        engine: Option<String>,
        #[arg(long)]
        role: Option<String>,
    },
    Show {
        id: String,
    },
}

#[derive(Subcommand)]
enum EngineCmd {
    List,
    /// Parameter metadata (ParamMeta) of an engine
    Params {
        engine_id: String,
    },
}

#[derive(Subcommand)]
enum SchemaCmd {
    /// Write transaction/project/preset schemas
    Export {
        #[arg(long, default_value = "schemas")]
        out_dir: PathBuf,
    },
}

#[derive(Subcommand)]
enum SongCmd {
    /// Create a source folder with a small playable song (pack it to get the .agbsong file)
    New {
        #[arg(long)]
        out: PathBuf,
        #[arg(long, default_value = "untitled")]
        title: String,
    },
    /// Check a song (.agbsong file or its source folder) against the format; report what this player skips
    Validate { path: PathBuf },
    /// Summarise a song: metadata, tracks, files, player support
    Info { path: PathBuf },
    /// Pack a source folder into a single .agbsong file (reproducible bytes)
    Pack {
        dir: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Unpack an .agbsong file into a new source folder
    Unpack {
        file: PathBuf,
        #[arg(long)]
        out_dir: PathBuf,
    },
    /// Play a song for the app: NDJSON events with one stem chunk per bar
    Play {
        path: PathBuf,
        #[arg(long)]
        work_dir: PathBuf,
        #[arg(long, default_value_t = 0, allow_hyphen_values = true)]
        transpose: i32,
        #[arg(long)]
        bpm: Option<f64>,
        /// Pace by the wall clock (live playback)
        #[arg(long)]
        realtime: bool,
        /// Answer the listener with the built-in offline performer
        #[arg(long)]
        mock: bool,
        /// Model(s) in fallback order, comma separated
        #[arg(long, value_delimiter = ',')]
        model: Vec<String>,
        #[arg(long)]
        base_url: Option<String>,
        /// File holding the API key (default: $AGENTLLM_API_KEY)
        #[arg(long)]
        api_key_file: Option<PathBuf>,
        #[arg(long, default_value_t = 2)]
        max_retries: u32,
    },
    /// Render a song to WAV; transposition and tempo are applied at play time
    Render {
        path: PathBuf,
        #[arg(long)]
        out_dir: PathBuf,
        /// Semitones, applied to transposable tracks
        #[arg(long, default_value_t = 0, allow_hyphen_values = true)]
        transpose: i32,
        /// Starting tempo; the tempo map scales with it
        #[arg(long)]
        bpm: Option<f64>,
        #[arg(long)]
        from_tick: Option<u64>,
        #[arg(long)]
        to_tick: Option<u64>,
        #[arg(long)]
        seed: Option<u64>,
        #[arg(long)]
        stems: bool,
        #[arg(long)]
        tail_ms: Option<u32>,
        #[arg(long, value_enum, default_value = "pcm16")]
        format: Format,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum Kind {
    Sample,
    Wavetable,
    Sfz,
    Other,
}

#[derive(Subcommand)]
enum AssetCmd {
    /// Copy a file into <project>/assets and register its license
    Add {
        #[arg(long)]
        project: PathBuf,
        #[arg(long)]
        file: PathBuf,
        #[arg(long)]
        id: String,
        /// SPDX id, LicenseRef-original or LicenseRef-private
        #[arg(long)]
        license: String,
        #[arg(long)]
        redistributable: bool,
        #[arg(long)]
        source: Option<String>,
        #[arg(long, value_enum, default_value = "sample")]
        kind: Kind,
    },
    /// Check files exist, hashes match, licenses registered
    Verify {
        #[arg(long)]
        project: PathBuf,
        /// Also enforce the factory license policy
        #[arg(long)]
        factory: bool,
    },
}

fn run(cli: Cli) -> CmdResult {
    match cli.cmd {
        Cmd::Project { cmd } => match cmd {
            ProjectCmd::Init { project, name } => project_init(&project, name.as_deref()),
            ProjectCmd::Snapshot { project, at_tick } => project_snapshot(&project, at_tick),
        },
        Cmd::Txn { cmd: TxnCmd::Apply { project, file, stdin, dry_run } } => {
            let raw = if stdin {
                let mut s = String::new();
                std::io::stdin().read_to_string(&mut s).map_err(|e| {
                    agb_protocol::ErrorResponse::new(None, agb_protocol::AgbError::io(e))
                })?;
                s
            } else {
                let f = file.expect("clap enforces --file or --stdin");
                std::fs::read_to_string(&f).map_err(|e| {
                    agb_protocol::ErrorResponse::new(
                        None,
                        agb_protocol::AgbError::io(format!("{}: {e}", f.display())),
                    )
                })?
            };
            txn_apply(&project, &raw, dry_run)
        }
        Cmd::Render {
            cmd: RenderCmd::Offline { project, from_tick, to_tick, seed, stems, tail_ms, format, out_dir, out },
        } => render_offline(
            &project,
            &RenderArgs {
                from_tick,
                to_tick,
                seed,
                stems,
                tail_ms,
                format: match format {
                    Format::Pcm16 => SampleFormat::Pcm16,
                    Format::F32 => SampleFormat::F32,
                },
                out_dir,
                out,
            },
        ),
        Cmd::Transport { cmd } => match cmd {
            TransportCmd::Set { project, tick } => transport_set(&project, tick),
            TransportCmd::Show { project } => transport_show(&project),
        },
        Cmd::Preset { cmd } => match cmd {
            PresetCmd::Search { tags, engine, role } => {
                preset_search(&tags, engine.as_deref(), role.as_deref())
            }
            PresetCmd::Show { id } => preset_show(&id),
        },
        Cmd::Engine { cmd } => match cmd {
            EngineCmd::List => engine_list(),
            EngineCmd::Params { engine_id } => engine_params(&engine_id),
        },
        Cmd::Schema { cmd: SchemaCmd::Export { out_dir } } => schema_export(&out_dir),
        Cmd::Asset { cmd } => match cmd {
            AssetCmd::Add { project, file, id, license, redistributable, source, kind } => asset_add(
                &project,
                &AssetAddArgs {
                    file,
                    id,
                    license,
                    redistributable,
                    source,
                    kind: match kind {
                        Kind::Sample => AssetKind::Sample,
                        Kind::Wavetable => AssetKind::Wavetable,
                        Kind::Sfz => AssetKind::Sfz,
                        Kind::Other => AssetKind::Other,
                    },
                },
            ),
            AssetCmd::Verify { project, factory } => asset_verify(&project, factory),
        },
        Cmd::Song { cmd } => match cmd {
            SongCmd::New { out, title } => song_new(&out, &title),
            SongCmd::Validate { path } => song_validate(&path),
            SongCmd::Info { path } => song_info(&path),
            SongCmd::Pack { dir, out } => song_pack(&dir, &out),
            SongCmd::Unpack { file, out_dir } => song_unpack(&file, &out_dir),
            SongCmd::Play { .. } => unreachable!("handled in main"),
            SongCmd::Render { path, out_dir, transpose, bpm, from_tick, to_tick, seed, stems, tail_ms, format } => {
                song_render(
                    &path,
                    &SongRenderArgs {
                        out_dir,
                        transpose,
                        bpm,
                        from_tick,
                        to_tick,
                        seed,
                        stems,
                        tail_ms,
                        format: match format {
                            Format::Pcm16 => SampleFormat::Pcm16,
                            Format::F32 => SampleFormat::F32,
                        },
                    },
                )
            }
        },
        Cmd::Oneshot { engine, preset, params, pitch, len_ms, tail_ms, out } => {
            oneshot(&engine, preset.as_deref(), params.as_deref(), &pitch, len_ms, tail_ms, &out)
        }
        Cmd::Perform { .. } => unreachable!("handled in main"),
    }
}

fn main() {
    let cli = Cli::parse();
    if let Cmd::Perform { project, brief, phrases, bars, realtime, seed, model, base_url, api_key_file, max_retries, continue_existing, mock } = &cli.cmd {
        let code = perform_cmd(PerformArgs {
            project: project.clone(),
            brief: brief.clone(),
            phrases: *phrases,
            bars: *bars,
            realtime: *realtime,
            seed: *seed,
            models: model.clone(),
            base_url: base_url.clone().or_else(|| std::env::var("AGENTLLM_API_BASE").ok()),
            api_key_file: api_key_file.clone(),
            max_retries: *max_retries,
            continue_existing: *continue_existing,
            mock: *mock,
        });
        std::process::exit(code);
    }
    if let Cmd::Song { cmd: SongCmd::Play { path, work_dir, transpose, bpm, realtime, mock, model, base_url, api_key_file, max_retries } } = &cli.cmd {
        let code = song_play(
            path,
            &SongPlayArgs {
                work_dir: work_dir.clone(),
                transpose: *transpose,
                bpm: *bpm,
                realtime: *realtime,
                mock: *mock,
                models: model.clone(),
                base_url: base_url.clone().or_else(|| std::env::var("AGENTLLM_API_BASE").ok()),
                api_key_file: api_key_file.clone(),
                max_retries: *max_retries,
            },
        );
        std::process::exit(code);
    }
    match run(cli) {
        Ok(v) => {
            println!("{}", serde_json::to_string_pretty(&v).unwrap());
            // Commands that report a failed check (e.g. `song validate`) still print the full report.
            if v.get("ok") == Some(&serde_json::Value::Bool(false)) {
                std::process::exit(1);
            }
        }
        Err(e) => {
            println!("{}", serde_json::to_string_pretty(&e).unwrap());
            std::process::exit(1);
        }
    }
}
