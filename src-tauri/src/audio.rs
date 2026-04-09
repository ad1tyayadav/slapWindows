use std::{
    fs::{self, File},
    io::BufReader,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        mpsc::{self, Sender},
        Arc,
    },
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink};

pub struct AudioController {
    bundled_root: PathBuf,
    custom_root: PathBuf,
    sender: Sender<AudioCommand>,
}

struct AudioCommand {
    pack_name: String,
    force: f32,
    playback_mode: PlaybackMode,
    selected_sound: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub enum PlaybackMode {
    Cycle,
    Random,
    Single,
}

impl AudioController {
    pub fn new(bundled_root: PathBuf, custom_root: PathBuf) -> Result<Self, String> {
        let (sender, receiver) = mpsc::channel::<AudioCommand>();
        let thread_bundled_root = bundled_root.clone();
        let thread_custom_root = custom_root.clone();

        thread::spawn(move || {
            let manager = match AudioManager::new(thread_bundled_root, thread_custom_root) {
                Ok(manager) => manager,
                Err(error) => {
                    eprintln!("audio init failed: {error}");
                    return;
                }
            };

            while let Ok(command) = receiver.recv() {
                if let Err(error) = manager.play_sound(
                    &command.pack_name,
                    command.force,
                    command.playback_mode,
                    command.selected_sound.as_deref(),
                ) {
                    eprintln!("audio playback failed: {error}");
                }
            }
        });

        Ok(Self {
            bundled_root,
            custom_root,
            sender,
        })
    }

    pub fn sound_packs(&self) -> Result<Vec<String>, String> {
        let mut packs = Vec::new();
        collect_pack_names(&self.bundled_root, &mut packs)?;
        collect_pack_names(&self.custom_root, &mut packs)?;
        packs.sort();
        packs.dedup();
        Ok(packs)
    }

    pub fn custom_pack_names(&self) -> Result<Vec<String>, String> {
        let mut packs = Vec::new();
        collect_pack_names(&self.custom_root, &mut packs)?;
        packs.sort();
        packs.dedup();
        Ok(packs)
    }

    pub fn is_custom_pack(&self, pack_name: &str) -> bool {
        self.custom_root.join(pack_name).is_dir()
    }

    pub fn import_sound_to_pack(
        &self,
        pack_name: &str,
        file_name: &str,
        bytes: &[u8],
    ) -> Result<String, String> {
        let sanitized_pack_name = sanitize_pack_name(pack_name);
        if sanitized_pack_name.is_empty() {
            return Err("invalid pack name".into());
        }

        let sanitized_name = sanitize_file_name(file_name);
        if sanitized_name.is_empty() {
            return Err("invalid file name".into());
        }

        if !is_supported_audio_file(Path::new(&sanitized_name)) {
            return Err("only .wav, .mp3, and .ogg files are supported".into());
        }

        let custom_pack_dir = self.custom_root.join(&sanitized_pack_name);
        fs::create_dir_all(&custom_pack_dir).map_err(|error| {
            format!(
                "failed to create custom sound directory '{}': {error}",
                custom_pack_dir.display()
            )
        })?;

        let target_path = unique_target_path(&custom_pack_dir, &sanitized_name);
        fs::write(&target_path, bytes).map_err(|error| {
            format!(
                "failed to write custom sound file '{}': {error}",
                target_path.display()
            )
        })?;

        Ok(sanitized_pack_name)
    }

    pub fn sound_files(&self, pack_name: &str) -> Result<Vec<String>, String> {
        let files = self.pack_files(pack_name)?;
        Ok(files
            .into_iter()
            .filter_map(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .map(|name| name.to_string())
            })
            .collect())
    }

    pub fn remove_custom_sound(&self, pack_name: &str, file_name: &str) -> Result<(), String> {
        let sanitized_pack_name = sanitize_pack_name(pack_name);
        if sanitized_pack_name.is_empty() {
            return Err("invalid pack name".into());
        }

        let sanitized_name = sanitize_file_name(file_name);
        if sanitized_name.is_empty() {
            return Err("invalid file name".into());
        }

        let custom_path = self.custom_root.join(&sanitized_pack_name).join(&sanitized_name);
        let bundled_path = self.bundled_root.join(&sanitized_pack_name).join(&sanitized_name);

        let target_path = if custom_path.exists() {
            custom_path
        } else if bundled_path.exists() {
            bundled_path
        } else {
            return Err(format!(
                "sound '{}' was not found in pack '{}'",
                sanitized_name, sanitized_pack_name
            ));
        };

        fs::remove_file(&target_path).map_err(|error| {
            format!(
                "failed to remove sound '{}': {error}",
                target_path.display()
            )
        })?;

        // Clean up empty directories in both roots
        for root in [&self.custom_root, &self.bundled_root] {
            let pack_dir = root.join(&sanitized_pack_name);
            if pack_dir.exists() {
                if let Ok(true) = directory_is_empty(&pack_dir) {
                    let _ = fs::remove_dir(&pack_dir);
                }
            }
        }

        Ok(())
    }

    pub fn play_sound(
        &self,
        pack_name: &str,
        force: f32,
        playback_mode: PlaybackMode,
        selected_sound: Option<String>,
    ) -> Result<(), String> {
        self.sender
            .send(AudioCommand {
                pack_name: pack_name.to_string(),
                force,
                playback_mode,
                selected_sound,
            })
            .map_err(|error| format!("failed to queue sound playback: {error}"))
    }

    fn pack_files(&self, pack_name: &str) -> Result<Vec<PathBuf>, String> {
        let mut files = Vec::new();
        collect_audio_files(&self.custom_root.join(pack_name), &mut files)?;
        collect_audio_files(&self.bundled_root.join(pack_name), &mut files)?;
        files.sort();
        Ok(files)
    }
}

struct AudioManager {
    _stream: OutputStream,
    handle: OutputStreamHandle,
    bundled_root: PathBuf,
    custom_root: PathBuf,
    next_index: Arc<AtomicUsize>,
    random_state: Arc<AtomicU64>,
    last_random_index: Arc<AtomicUsize>,
}

impl AudioManager {
    fn new(bundled_root: PathBuf, custom_root: PathBuf) -> Result<Self, String> {
        let (stream, handle) = OutputStream::try_default()
            .map_err(|error| format!("failed to open audio output: {error}"))?;
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| format!("failed to initialize random seed: {error}"))?
            .as_nanos() as u64;

        Ok(Self {
            _stream: stream,
            handle,
            bundled_root,
            custom_root,
            next_index: Arc::new(AtomicUsize::new(0)),
            random_state: Arc::new(AtomicU64::new(seed.max(1))),
            last_random_index: Arc::new(AtomicUsize::new(usize::MAX)),
        })
    }

    fn play_sound(
        &self,
        pack_name: &str,
        force: f32,
        playback_mode: PlaybackMode,
        selected_sound: Option<&str>,
    ) -> Result<(), String> {
        let files = self.pack_files(pack_name)?;
        if files.is_empty() {
            return Err(format!("sound pack '{pack_name}' has no playable files"));
        }

        let path = select_audio_file(
            &files,
            playback_mode,
            selected_sound,
            &self.next_index,
            &self.random_state,
            &self.last_random_index,
        )?;
        let file = File::open(path)
            .map_err(|error| format!("failed to open sound file '{}': {error}", path.display()))?;
        let decoder = Decoder::new(BufReader::new(file))
            .map_err(|error| format!("failed to decode sound file '{}': {error}", path.display()))?;

        let sink = Sink::try_new(&self.handle)
            .map_err(|error| format!("failed to create playback sink: {error}"))?;
        sink.set_volume(force.sqrt().clamp(0.05, 1.0));
        sink.append(decoder);
        sink.detach();
        Ok(())
    }

    fn pack_files(&self, pack_name: &str) -> Result<Vec<PathBuf>, String> {
        let mut files = Vec::new();
        collect_audio_files(&self.custom_root.join(pack_name), &mut files)?;
        collect_audio_files(&self.bundled_root.join(pack_name), &mut files)?;
        files.sort();
        Ok(files)
    }
}

fn collect_pack_names(root: &Path, packs: &mut Vec<String>) -> Result<(), String> {
    if !root.exists() {
        return Ok(());
    }

    let entries =
        fs::read_dir(root).map_err(|error| format!("failed to read sound packs: {error}"))?;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() && directory_has_audio_files(&path)? {
            if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
                packs.push(name.to_string());
            }
        }
    }

    Ok(())
}

fn collect_audio_files(root: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    if !root.exists() {
        return Ok(());
    }

    let entries = fs::read_dir(root).map_err(|error| {
        format!(
            "failed to read sound pack directory '{}': {error}",
            root.display()
        )
    })?;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() && is_supported_audio_file(&path) {
            files.push(path);
        }
    }

    Ok(())
}

fn directory_has_audio_files(root: &Path) -> Result<bool, String> {
    if !root.exists() {
        return Ok(false);
    }

    let entries = fs::read_dir(root).map_err(|error| {
        format!(
            "failed to read sound pack directory '{}': {error}",
            root.display()
        )
    })?;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() && is_supported_audio_file(&path) {
            return Ok(true);
        }
    }

    Ok(false)
}

fn directory_is_empty(root: &Path) -> Result<bool, String> {
    let mut entries = fs::read_dir(root)
        .map_err(|error| format!("failed to inspect directory '{}': {error}", root.display()))?;
    Ok(entries.next().is_none())
}

fn is_supported_audio_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("wav") | Some("mp3") | Some("ogg") | Some("WAV") | Some("MP3") | Some("OGG")
    )
}

fn sanitize_file_name(file_name: &str) -> String {
    file_name
        .chars()
        .map(|character| match character {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '.' | '-' | '_' => character,
            _ => '_',
        })
        .collect()
}

fn sanitize_pack_name(pack_name: &str) -> String {
    pack_name
        .trim()
        .chars()
        .map(|character| match character {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => character,
            ' ' => '-',
            _ => '-',
        })
        .collect::<String>()
        .trim_matches('-')
        .to_lowercase()
}

fn unique_target_path(parent: &Path, file_name: &str) -> PathBuf {
    let candidate = parent.join(file_name);
    if !candidate.exists() {
        return candidate;
    }

    let stem = Path::new(file_name)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("sound");
    let extension = Path::new(file_name)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("");

    for index in 1.. {
        let numbered_name = if extension.is_empty() {
            format!("{stem}-{index}")
        } else {
            format!("{stem}-{index}.{extension}")
        };
        let path = parent.join(numbered_name);
        if !path.exists() {
            return path;
        }
    }

    unreachable!("unique path generation should always return");
}

fn select_audio_file<'a>(
    files: &'a [PathBuf],
    playback_mode: PlaybackMode,
    selected_sound: Option<&str>,
    next_index: &AtomicUsize,
    random_state: &AtomicU64,
    last_random_index: &AtomicUsize,
) -> Result<&'a PathBuf, String> {
    match playback_mode {
        PlaybackMode::Cycle => {
            let index = next_index.fetch_add(1, Ordering::Relaxed) % files.len();
            Ok(&files[index])
        }
        PlaybackMode::Random => {
            let mut state = next_random_u64(random_state);
            let mut index = random_index_from_state(state, files.len());

            if files.len() > 1 {
                let last_index = last_random_index.load(Ordering::Relaxed);
                if index == last_index {
                    state = next_random_u64(random_state);
                    index = random_index_from_state(state, files.len());

                    if index == last_index {
                        index = (last_index + 1) % files.len();
                    }
                }
            }

            last_random_index.store(index, Ordering::Relaxed);
            Ok(&files[index])
        }
        PlaybackMode::Single => {
            let selected_sound = selected_sound.ok_or("no sound selected for single mode")?;
            files
                .iter()
                .find(|path| {
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name == selected_sound)
                })
                .ok_or_else(|| format!("selected sound '{selected_sound}' was not found"))
        }
    }
}

fn next_random_u64(random_state: &AtomicU64) -> u64 {
    let previous = random_state
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
            Some(value.wrapping_mul(6364136223846793005).wrapping_add(1))
        })
        .unwrap_or(1);

    previous
        .wrapping_mul(0xff51afd7ed558ccd)
        .rotate_left(17)
        .wrapping_mul(0xc4ceb9fe1a85ec53)
}

fn random_index_from_state(state: u64, len: usize) -> usize {
    (((state >> 32) ^ state) as usize) % len
}
