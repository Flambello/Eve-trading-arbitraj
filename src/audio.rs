#[cfg(not(test))]
use std::path::Path;
use std::process::Command;
use std::thread;

/// Play an audible notification alert asynchronously in a background thread
/// when a new 100%+ profit trade is discovered.
pub fn play_high_profit_alert() {
    thread::spawn(|| {
        play_sound_internal("message-new-instant");
    });
}

/// Play a sample sound to test audio configuration from the UI.
pub fn play_test_sound() {
    thread::spawn(|| {
        play_sound_internal("bell");
    });
}

/// Send a desktop notification banner asynchronously when a high-profit trade is found.
pub fn send_desktop_notification(summary: &str, body: &str) {
    let summary = summary.to_string();
    let body = body.to_string();
    thread::spawn(move || {
        let _ = Command::new("notify-send")
            .args([
                "-a",
                "EVE Traveling Merchant",
                "-u",
                "normal",
                &summary,
                &body,
            ])
            .status();
    });
}

/// Check if any supported system audio player binary is installed in the given PATH variable.
pub fn check_audio_support_in_paths(path_var: &std::ffi::OsStr) -> bool {
    let players = ["canberra-gtk-play", "pw-play", "paplay", "aplay"];
    for dir in std::env::split_paths(path_var) {
        for player in &players {
            let full_path = dir.join(player);
            if full_path.is_file() {
                return true;
            }
        }
    }
    false
}

/// Check if any supported system audio player binary is installed on the system.
pub fn has_audio_support() -> bool {
    if let Some(path_var) = std::env::var_os("PATH") {
        check_audio_support_in_paths(&path_var)
    } else {
        false
    }
}

fn play_sound_internal(sound_theme_id: &str) {
    #[cfg(test)]
    {
        // Guard against spawning external processes and emitting terminal bells in unit tests
        assert!(!sound_theme_id.is_empty(), "Sound theme ID must not be empty");
    }

    #[cfg(not(test))]
    {
        // 1. Canberra GTK play (Freedesktop system sound event)
        if let Ok(status) = Command::new("canberra-gtk-play")
            .args(["-i", sound_theme_id, "-d", "EVE Traveling Merchant"])
            .status()
        {
            if status.success() {
                return;
            }
        }

        // 2. PipeWire native player (pw-play) with freedesktop stereo oga
        let oga_path = format!(
            "/usr/share/sounds/freedesktop/stereo/{}.oga",
            sound_theme_id
        );
        if Path::new(&oga_path).exists() {
            if let Ok(status) = Command::new("pw-play").arg(&oga_path).status() {
                if status.success() {
                    return;
                }
            }

            // 3. PulseAudio player (paplay)
            if let Ok(status) = Command::new("paplay").arg(&oga_path).status() {
                if status.success() {
                    return;
                }
            }
        }

        // 4. Terminal ASCII Bell fallback
        eprint!("\x07");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    #[test]
    fn test_audio_support_detection() {
        // 1. Empty or nonexistent path must return false
        assert!(!check_audio_support_in_paths(OsStr::new("")));
        assert!(!check_audio_support_in_paths(OsStr::new("/tmp/nonexistent_audio_path_xyz")));

        // 2. has_audio_support returns a valid bool without panicking
        let supported = has_audio_support();
        let _ = supported; // Either true or false depending on host system
    }

    #[test]
    fn test_play_sound_does_not_panic() {
        play_sound_internal("message-new-instant");
        play_sound_internal("bell");
        play_high_profit_alert();
        play_test_sound();
    }
}
