use std::sync::{Arc, Mutex};

use gpui::{Image, ImageFormat};
use lru::LruCache;
use mdict_core::DictionaryStore;

#[derive(Clone)]
pub struct ImageCache {
    inner: Arc<Mutex<LruCache<String, Option<Arc<Image>>>>>,
}

impl Default for ImageCache {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(LruCache::new(
                std::num::NonZeroUsize::new(120).unwrap(),
            ))),
        }
    }
}

impl ImageCache {
    pub fn get(&self, store: &Arc<DictionaryStore>, dict_id: &str, path: &str) -> Option<Arc<Image>> {
        let key = format!("{}:{}", dict_id, path);
        if let Some(cached) = self.inner.lock().unwrap().get(&key) {
            return cached.clone();
        }
        let image = load_image(store, dict_id, path);
        self.inner.lock().unwrap().put(key, image.clone());
        image
    }
}

fn load_image(store: &Arc<DictionaryStore>, dict_id: &str, path: &str) -> Option<Arc<Image>> {
    let (bytes, _) = store.read_resource(dict_id, path).ok().flatten()?;
    let lower = path.to_ascii_lowercase();
    let format = if lower.ends_with(".png") {
        ImageFormat::Png
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        ImageFormat::Jpeg
    } else if lower.ends_with(".webp") {
        ImageFormat::Webp
    } else if lower.ends_with(".gif") {
        ImageFormat::Gif
    } else if lower.ends_with(".svg") {
        ImageFormat::Svg
    } else if lower.ends_with(".bmp") {
        ImageFormat::Bmp
    } else if lower.ends_with(".tif") || lower.ends_with(".tiff") {
        ImageFormat::Tiff
    } else {
        return None;
    };
    Some(Arc::new(Image::from_bytes(format, bytes)))
}

fn audio_candidates(path: &str) -> Vec<String> {
    if path.to_ascii_lowercase().contains("://") {
        return vec![path.to_string()];
    }
    let clean = path.trim_start_matches('/').replace('\\', "/");
    if std::path::Path::new(&clean).extension().is_some() {
        return vec![clean];
    }
    ["wav", "mp3", "ogg", "m4a", "flac"]
        .iter()
        .map(|ext| format!("{clean}.{ext}"))
        .collect()
}

fn audio_bytes(store: &Arc<DictionaryStore>, dict_id: &str, path: &str) -> Result<Vec<u8>, String> {
    let mut found = None;
    for candidate in audio_candidates(path) {
        if let Ok(Some((bytes, _))) = store.read_resource(dict_id, &candidate) {
            found = Some(bytes);
            break;
        }
    }
    let mut bytes = found.ok_or_else(|| "未找到发音文件".to_string())?;
    if bytes.starts_with(b"OggS") {
        if let Some(wav) = mdict_core::audio::spx_to_wav(&bytes) {
            bytes = wav;
        }
    }
    Ok(bytes)
}

#[cfg(any(windows, target_os = "macos"))]
mod platform {
    use std::{io::Cursor, sync::Arc};

    use mdict_core::DictionaryStore;

    use super::audio_bytes;

    pub struct AudioPlayer {
        stream: Option<(rodio::OutputStream, rodio::OutputStreamHandle)>,
        sinks: Vec<rodio::Sink>,
    }

    impl Default for AudioPlayer {
        fn default() -> Self {
            Self::new()
        }
    }

    impl AudioPlayer {
        pub fn new() -> Self {
            Self {
                stream: None,
                sinks: Vec::new(),
            }
        }

        pub fn play(
            &mut self,
            store: &Arc<DictionaryStore>,
            dict_id: &str,
            path: &str,
        ) -> Result<(), String> {
            let bytes = audio_bytes(store, dict_id, path)?;
            if self.stream.is_none() {
                let (stream, handle) = rodio::OutputStream::try_default()
                    .map_err(|e| format!("无法打开音频设备: {e}"))?;
                self.stream = Some((stream, handle));
            }
            let (_, handle) = self.stream.as_ref().unwrap();
            let sink = rodio::Sink::try_new(handle).map_err(|e| format!("无法播放音频: {e}"))?;
            let source = rodio::Decoder::new(Cursor::new(bytes))
                .map_err(|e| format!("不支持的音频格式: {e}"))?;
            sink.append(source);
            sink.play();
            self.sinks.retain(|s| !s.empty());
            self.sinks.push(sink);
            Ok(())
        }
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod platform {
    use std::sync::Arc;

    use mdict_core::DictionaryStore;

    use super::audio_bytes;

    pub struct AudioPlayer;

    impl Default for AudioPlayer {
        fn default() -> Self {
            Self::new()
        }
    }

    impl AudioPlayer {
        pub fn new() -> Self {
            Self
        }

        pub fn play(
            &mut self,
            store: &Arc<DictionaryStore>,
            dict_id: &str,
            path: &str,
        ) -> Result<(), String> {
            audio_bytes(store, dict_id, path)?;
            Err("当前平台暂不支持内置发音，请使用外部播放器".to_string())
        }
    }
}

pub use platform::AudioPlayer;
