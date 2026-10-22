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

/// Validate that a sound theme identifier only contains safe alphanumeric and hyphen/underscore characters.
pub fn is_safe_sound_theme_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
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
                "--",
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

/// Builds command arguments for canberra-gtk-play if the sound theme ID is safe.
pub fn build_canberra_command_args(sound_theme_id: &str) -> Option<Vec<String>> {
    if !is_safe_sound_theme_id(sound_theme_id) {
        return None;
    }
    Some(vec![
        "-i".to_string(),
        sound_theme_id.to_string(),
        "-d".to_string(),
        "EVE Traveling Merchant".to_string(),
    ])
}

fn play_sound_internal(sound_theme_id: &str) -> bool {
    let args = match build_canberra_command_args(sound_theme_id) {
        Some(a) => a,
        None => return false,
    };

    #[cfg(unix)]
    {
        // 1. Canberra GTK play (Freedesktop system sound event)
        if let Ok(status) = Command::new("canberra-gtk-play").args(&args).status() {
            if status.success() {
                return true;
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
                    return true;
                }
            }

            // 3. PulseAudio player (paplay)
            if let Ok(status) = Command::new("paplay").arg(&oga_path).status() {
                if status.success() {
                    return true;
                }
            }
        }
    }

    #[cfg(windows)]
    {
        // On Windows, play standard Asterisk alert sound via PowerShell
        if let Ok(mut child) = Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                "[System.Media.SystemSounds]::Asterisk.Play()",
            ])
            .spawn()
        {
            if let Ok(status) = child.wait() {
                if status.success() {
                    return true;
                }
            }
        }
    }

    // Terminal ASCII Bell fallback across all platforms
    tracing::warn!(
        error_code = %crate::error::ErrorCode::ErrUiAudioPlayback,
        sound_theme_id = sound_theme_id,
        "System audio player commands failed; fell back to terminal bell"
    );
    eprint!("\x07");
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    #[test]
    fn test_audio_support_detection() {
        // 1. Empty or nonexistent path must return false
        assert!(!check_audio_support_in_paths(OsStr::new("")));
        assert!(!check_audio_support_in_paths(OsStr::new(
            "/tmp/nonexistent_audio_path_xyz"
        )));

        // 2. Temp directory with mocked player binary returns true
        let temp_dir = std::env::temp_dir().join(format!("eve_audio_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        assert!(!check_audio_support_in_paths(temp_dir.as_os_str()));

        let dummy_bin = temp_dir.join("canberra-gtk-play");
        std::fs::write(&dummy_bin, b"#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&dummy_bin).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&dummy_bin, perms).unwrap();
        }
        assert!(
            check_audio_support_in_paths(temp_dir.as_os_str()),
            "Directory containing executable canberra-gtk-play must report audio support"
        );
        let _ = std::fs::remove_dir_all(&temp_dir);

        // 3. has_audio_support matches actual PATH environment check
        let expected = std::env::var_os("PATH")
            .map(|p| check_audio_support_in_paths(&p))
            .unwrap_or(false);
        assert_eq!(has_audio_support(), expected);
    }

    #[test]
    fn test_play_sound_command_generation_and_safety() {
        // Valid sound themes produce correct command args
        let args = build_canberra_command_args("message-new-instant")
            .expect("Valid theme must produce args");
        assert_eq!(
            args,
            vec!["-i", "message-new-instant", "-d", "EVE Traveling Merchant"]
        );

        let bell_args = build_canberra_command_args("bell").expect("Bell theme must produce args");
        assert_eq!(
            bell_args,
            vec!["-i", "bell", "-d", "EVE Traveling Merchant"]
        );

        // Malicious or invalid themes return None
        assert!(build_canberra_command_args("../../../etc/passwd").is_none());
        assert!(build_canberra_command_args("bell; rm -rf /").is_none());
        assert!(build_canberra_command_args("").is_none());
        assert!(build_canberra_command_args("   ").is_none());

        // Invalid themes safely rejected by production playback function
        assert!(!play_sound_internal("../../../etc/passwd"));
        assert!(!play_sound_internal("bell; rm -rf /"));
        assert!(!play_sound_internal(""));

        // Valid theme executed through production playback pipeline
        assert!(play_sound_internal("bell"));

        // Play sound functions execute safely without panicking
        play_high_profit_alert();
        play_test_sound();
    }
}
