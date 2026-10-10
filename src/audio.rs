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
            .args(["-a", "EVE Traveling Merchant", "-u", "normal", &summary, &body])
            .status();
    });
}

/// Check if any supported system audio player binary is installed.
pub fn has_audio_support() -> bool {
    let players = ["canberra-gtk-play", "pw-play", "paplay", "aplay"];
    players.iter().any(|cmd| {
        Command::new("which")
            .arg(cmd)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    })
}

fn play_sound_internal(sound_theme_id: &str) {
    // 1. Canberra GTK play (Freedesktop system sound event)
    if let Ok(status) = Command::new("canberra-gtk-play")
        .args(["-i", sound_theme_id, "-d", "EVE Trade Scout"])
        .status()
    {
        if status.success() {
            return;
        }
    }

    // 2. PipeWire native player (pw-play) with freedesktop stereo oga
    let oga_path = format!("/usr/share/sounds/freedesktop/stereo/{}.oga", sound_theme_id);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audio_support_detection() {
        // On modern Linux systems (like Fedora), audio utilities are present
        let supported = has_audio_support();
        assert!(supported, "Expected system audio player to be detected");
    }

    #[test]
    fn test_play_sound_does_not_panic() {
        // Verification that dispatching sound does not panic
        play_sound_internal("message-new-instant");
    }
}
