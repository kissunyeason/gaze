// SPDX-FileCopyrightText: 2026 Gundu Labs
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

pub(super) fn desktop_name() -> String {
    let from_env = [
        std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default(),
        std::env::var("XDG_SESSION_DESKTOP").unwrap_or_default(),
        std::env::var("DESKTOP_SESSION").unwrap_or_default(),
    ]
    .join(":")
    .to_ascii_lowercase();
    if from_env.chars().any(|c| c != ':') {
        return from_env;
    }
    // `sudo` strips those, so fall back to what is running: otherwise
    // `sudo gaze doctor` silently drops every desktop check.
    desktop_from_processes(owning_uid())
}

pub(super) fn running_as_root() -> bool {
    unsafe { libc::geteuid() == 0 }
}

/// The user whose session is being checked: the invoking user under `sudo`.
pub(super) fn owning_uid() -> u32 {
    std::env::var("SUDO_UID")
        .ok()
        .and_then(|uid| uid.parse().ok())
        .unwrap_or_else(|| unsafe { libc::getuid() })
}

/// Returns colon-delimited desktop names for `contains` checks.
/// The CLI does not link pam-gaze.
pub(super) fn desktop_from_processes(uid: u32) -> String {
    use std::os::unix::fs::MetadataExt;

    let Ok(entries) = fs::read_dir("/proc") else {
        return String::new();
    };
    let mut found = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !entry.metadata().is_ok_and(|meta| meta.uid() == uid) {
            continue;
        }
        let Ok(comm) = fs::read_to_string(path.join("comm")) else {
            continue;
        };
        let name = match comm.trim() {
            "plasmashell" | "kwin_wayland" | "kwin_x11" => "kde",
            "gnome-shell" => "gnome",
            "Hyprland" | "hyprland" => "hyprland",
            _ => continue,
        };
        if !found.contains(&name) {
            found.push(name);
        }
    }
    found.join(":")
}

/// Names the exact preferences page and group, making it easier for users to find.
pub(super) fn gnome_prefs_path(group: &str, switch: &str) -> String {
    format!(
        "Open it with `gnome-extensions prefs {GNOME_EXTENSION_ID}` (or the Extensions app, then Gaze), then Behavior -> {group} -> \"{switch}\""
    )
}

/// GNOME Shell scans extension directories when a session starts. If asked to enable
/// an unseen UUID, it drops that UUID the next time it rewrites `enabled-extensions`.
pub(super) fn gnome_extension_enable_steps() -> String {
    format!(
        "1. Reboot, or log out and back in, so GNOME Shell scans the extension.\n\
         2. Run `gnome-extensions enable {GNOME_EXTENSION_ID}`.\n\
         3. Run `gsettings set {GNOME_EXTENSION_SCHEMA} enable-face-authentication true`.\n\
         If step 2 reports that the extension does not exist, Shell has not rescanned yet: reboot and repeat.\n\
         Details: {GNOME_DOCS_URL}"
    )
}

pub(super) fn check_desktop_integration(report: &mut Report) {
    let desktop = desktop_name();
    if desktop.contains("gnome") {
        match command_output("gnome-extensions", &["list", "--enabled"]) {
            Ok((true, output)) if output.lines().any(|line| line.trim() == GNOME_EXTENSION_ID) => {
                report.pass("GNOME 扩展", "已为当前用户启用");
            }
            Ok((true, _)) if extension_installed() => report.warning(
                "GNOME 扩展",
                "已安装，但未对当前用户启用",
                gnome_extension_enable_steps(),
            ),
            Ok((true, _)) => report.warning(
                "GNOME 扩展",
                "未对当前用户安装",
                format!(
                    "Install the Gaze GNOME extension package (`gaze-gnome-extension`), reboot, then run `gnome-extensions enable {GNOME_EXTENSION_ID}`. See {GNOME_DOCS_URL}"
                ),
            ),
            Ok((false, message)) => report.warning(
                "GNOME 扩展",
                format!("无法查询扩展：{message}"),
                "请确认 GNOME Shell 正在运行，并重装 Gaze GNOME 扩展包。",
            ),
            Err(err) => report.warning(
                "GNOME 扩展",
                format!("无法查询扩展：{err}"),
                "请安装 Gaze GNOME 扩展包以支持锁屏认证。",
            ),
        }

        match extension_setting("enable-face-authentication") {
            Ok((true, value)) if value == "true" => {
                report.pass(
                    "GNOME 锁屏人脸认证",
                    "已为当前用户启用",
                );
            }
            Ok((true, _)) => report.off(
                "GNOME 锁屏人脸认证",
                "对当前用户已关闭，锁屏只接受密码",
                format!(
                    "Turn it on: {}.\n\
                     From a terminal: `dconf write /org/gnome/shell/extensions/gaze/enable-face-authentication true`.\n\
                     (`gsettings set {GNOME_EXTENSION_SCHEMA} ...` does the same, but cannot find the schema where it ships inside the extension directory, as on NixOS.)",
                    gnome_prefs_path("人脸认证", "Enable face authentication (lock screen)")
                ),
            ),
            Ok((false, message)) => report.warning(
                "GNOME 锁屏人脸认证",
                format!("无法读取扩展设置：{message}"),
                "请重装 Gaze GNOME 扩展包。",
            ),
            Err(err) => report.warning(
                "GNOME 锁屏人脸认证",
                format!("无法读取扩展设置：{err}"),
                "请重装 Gaze GNOME 扩展包。",
            ),
        }

        let override_exists = Path::new(GDM_FACE_OVERRIDE_PATH).exists();
        let dconf_face_auth = gdm_face_auth_from_dconf();
        match (dconf_face_auth, override_exists) {
            (Some(false), true) => report.warning(
                "GDM 登录人脸认证",
                format!(
                    "{GDM_FACE_OVERRIDE_PATH} enables it, but the compiled GDM dconf database still reports it disabled"
                ),
                "请运行 `sudo dconf update`，然后重启 GDM（或重启系统）。",
            ),
            (Some(true), false) => report.pass(
                "GDM 登录人脸认证",
                "enabled in the GDM dconf profile by your system configuration, not by Gaze (on NixOS, `services.gaze.gnome.gdmFaceLogin`)",
            ),
            (_, true) => match gdm_greeter_readiness() {
                GdmGreeterReadiness::Ready => report.pass(
                    "GDM 登录人脸认证",
                    format!(
                        "enabled system-wide via {GDM_FACE_OVERRIDE_PATH}; toggle it under Behavior -> GDM login screen in `gnome-extensions prefs {GNOME_EXTENSION_ID}`"
                    ),
                ),
                GdmGreeterReadiness::ProfileMissingSystemDb(profile) => report.error(
                    "GDM 登录人脸认证",
                    format!(
                        "{GDM_FACE_OVERRIDE_PATH} exists, but the greeter's dconf profile {profile} does not list `system-db:{GDM_DCONF_PROFILE}`, so GDM never reads it"
                    ),
                    format!(
                        "Reinstall the Gaze GNOME extension package, or add a `system-db:{GDM_DCONF_PROFILE}` line after `user-db:user` in {profile} (copying it from {VENDOR_DCONF_PROFILE_DIR} first if it does not exist), run `sudo dconf update`, then reboot."
                    ),
                ),
                GdmGreeterReadiness::CompiledDbMissing => report.error(
                    "GDM 登录人脸认证",
                    format!(
                        "{GDM_FACE_OVERRIDE_PATH} exists, but the compiled database {GDM_COMPILED_DB_PATH} does not"
                    ),
                    "请运行 `sudo dconf update`，然后重启系统。",
                ),
                GdmGreeterReadiness::ExtensionNotEnabled => report.error(
                    "GDM 登录人脸认证",
                    format!(
                        "the GDM database does not enable {GNOME_EXTENSION_ID} for the greeter, so the login screen never starts the {GDM_FACE_PAM_SERVICE} PAM service"
                    ),
                    "请重装 Gaze GNOME 扩展包，运行 `sudo dconf update`，然后重启系统。",
                ),
                GdmGreeterReadiness::ExtensionsDisabled(source) => report.error(
                    "GDM 登录人脸认证",
                    format!(
                        "the greeter resolves `org.gnome.shell disable-user-extensions` to true, which switches off every GNOME Shell extension at the login screen, {GNOME_EXTENSION_ID} included"
                    ),
                    match source {
                        Some(path) => format!(
                            "{} holds that key and outranks every keyfile under /etc/dconf/db/gdm.d, so it has to be cleared there:\n\
                             sudo rm -f {}\n\
                             Then reboot. GDM writes the file again with its own defaults.",
                            path.display(),
                            path.display()
                        ),
                        None => format!(
                            "Put `disable-user-extensions=false` under `[org/gnome/shell]` in {GDM_FACE_OVERRIDE_PATH}, run `sudo dconf update`, then reboot."
                        ),
                    },
                ),
                GdmGreeterReadiness::Unverifiable(why) => report.warning(
                    "GDM 登录人脸认证",
                    format!(
                        "{GDM_FACE_OVERRIDE_PATH} enables it, but the greeter configuration could not be verified: {why}"
                    ),
                    "请安装 `dconf` 命令行工具，然后重新运行 `gaze doctor`。",
                ),
            },
            (_, false) => report.off(
                "GDM 登录人脸认证",
                "已关闭，登录界面只接受密码（锁屏是另一个独立开关）",
                format!(
                    "Turn it on: {}, then reboot. It asks for admin authorization and writes {GDM_FACE_OVERRIDE_PATH} for you.\n\
                     By hand: put `enable-face-authentication=true` under `[org/gnome/shell/extensions/gaze]` in {GDM_FACE_OVERRIDE_PATH}, run `sudo dconf update`, then reboot.\n\
                     Details: {GNOME_DOCS_URL}#optional-enable-face-at-gdm-login",
                    gnome_prefs_path("GDM 登录界面", "Enable face authentication at GDM login")
                ),
            ),
        }

        if dconf_face_auth == Some(true) || override_exists {
            check_gdm_selinux(report);
        }
    }

    let omarchy = desktop.contains("hyprland")
        && Path::new("/usr/share/omarchy/shell/plugins/lock/manifest.json").exists();
    if omarchy {
        match command_output("gaze-omarchy", &["doctor"]) {
            Ok((true, output)) => report.pass("Omarchy 锁屏", output),
            Ok((false, output)) => report.warning(
                "Omarchy 锁屏",
                output,
                "Run `gaze-omarchy enable` from your unlocked desktop. See https://gaze.gundulabs.com/guide/omarchy",
            ),
            Err(_) => report.warning(
                "Omarchy 锁屏",
                "未安装 Gaze 的 Omarchy 集成",
                "请安装 `gaze-omarchy`（Arch 上为 `gaze-omarchy-bin`），然后以非 sudo 方式运行 `gaze-omarchy enable`。",
            ),
        }
    }
    if desktop.contains("hyprland") && !omarchy {
        let config_home = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")));
        let config_path = config_home.map(|home| home.join("hypr/hyprlock.conf"));
        let configured = config_path
            .as_ref()
            .and_then(|path| fs::read_to_string(path).ok())
            .is_some_and(|contents| hyprlock_selects_gaze(&contents));
        if configured {
            report.pass("hyprlock", "已配置使用 Gaze 的 PAM 服务");
        } else {
            report.warning(
                "hyprlock",
                "当前用户的 hyprlock.conf 未选择 Gaze 的 PAM 服务",
                "请在 hyprlock 的 `auth { pam { ... } }` 块中设置 `module = hyprlock-gaze`。",
            );
        }
    }

    if desktop.contains("kde") || desktop.contains("plasma") {
        check_kde_lock_screen(
            report,
            read_pam_service(KDE_FACE_PAM_FILE).as_deref(),
            read_pam_service(KDE_SMARTCARD_PAM_FILE).as_deref(),
            kscreenlocker_starts_smartcard_slot(),
        );
        check_kde_login_greeter(
            report,
            read_pam_service(PLASMALOGIN_FACE_PAM_FILE).as_deref(),
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum KdeLockStatus {
    Wired,
    /// Simultaneous mode waits for a response that this slot cannot provide.
    Grosshack,
    NotWired,
    /// KScreenLocker has no biometric slot configured at all.
    NoService,
}

pub(super) fn slot_status(slot: Option<&str>) -> KdeLockStatus {
    let Some(contents) = slot else {
        return KdeLockStatus::NoService;
    };
    let auth_lines = || {
        contents.lines().filter(|line| {
            matches!(
                line.split('#')
                    .next()
                    .unwrap_or_default()
                    .split_whitespace()
                    .next(),
                // `-auth` is what the helper writes: a missing module is then
                // skipped instead of aborting the greeter's stack.
                Some("auth") | Some("-auth")
            )
        })
    };
    if auth_lines().any(|line| {
        line.contains("pam_gaze_grosshack.so")
            || (pam_line_has_reference(line)
                && line.split_whitespace().any(|tok| tok == "simultaneous"))
    }) {
        return KdeLockStatus::Grosshack;
    }
    if auth_lines().any(pam_line_has_reference) {
        return KdeLockStatus::Wired;
    }
    KdeLockStatus::NotWired
}

/// Reports the slot with the most useful status. Either slot is sufficient when it
/// is wired to start Gaze before the user submits anything.
pub(super) fn kde_lock_status(
    kde_fingerprint: Option<&str>,
    kde_smartcard: Option<&str>,
) -> (KdeLockStatus, &'static str) {
    let slots = [
        (KDE_FACE_PAM_FILE, slot_status(kde_fingerprint)),
        (KDE_SMARTCARD_PAM_FILE, slot_status(kde_smartcard)),
    ];
    for wanted in [
        KdeLockStatus::Wired,
        KdeLockStatus::Grosshack,
        KdeLockStatus::NotWired,
    ] {
        if let Some((file, status)) = slots.iter().find(|(_, status)| *status == wanted) {
            return (*status, file);
        }
    }
    (KdeLockStatus::NoService, KDE_FACE_PAM_FILE)
}

pub(super) fn check_kde_lock_screen(
    report: &mut Report,
    kde_fingerprint: Option<&str>,
    kde_smartcard: Option<&str>,
    smartcard_up_front: bool,
) {
    const NAME: &str = "KDE 锁屏";
    let (status, file) = if smartcard_up_front {
        kde_lock_status(kde_fingerprint, kde_smartcard)
    } else {
        kde_lock_status(kde_fingerprint, None)
    };
    if status != KdeLockStatus::Wired
        && !smartcard_up_front
        && slot_status(kde_smartcard) == KdeLockStatus::Wired
    {
        report.warning(
            NAME,
            "kde-smartcard runs Gaze, but Plasma 6.8 and later only start that slot once it is picked as the unlock method, so face unlock never starts on its own",
            "请运行 `sudo gaze-kde-pam enable`，它会把 Gaze 迁移到 kde-fingerprint。",
        );
        return;
    }
    let slot = file.trim_start_matches("/etc/pam.d/");
    match status {
        KdeLockStatus::Wired => report.pass(
            NAME,
            format!(
                "{slot} runs Gaze, so face unlock starts on its own next to the password field"
            ),
        ),
        KdeLockStatus::Grosshack => report.warning(
            NAME,
            format!("{slot} runs pam_gaze.so in simultaneous mode, which waits for a password prompt that KScreenLocker can never answer"),
            format!("Use sequential mode there: replace it with `-auth [success=done default=ignore] pam_gaze.so` in {file}, or reinstall gaze-kde."),
        ),
        KdeLockStatus::NotWired => report.warning(
            NAME,
            format!("{file} does not run Gaze, so face authentication only starts after you submit the password field"),
            "请安装 gaze-kde 软件包，或运行 `sudo gaze-kde-pam enable`。",
        ),
        KdeLockStatus::NoService => report.warning(
            NAME,
            format!("{file} does not exist, so KScreenLocker has no biometric slot to start"),
            "请安装 gaze-kde 软件包，或运行 `sudo gaze-kde-pam enable` 以创建该文件。",
        ),
    }
}

/// Pre-submit scanning requires a separate biometric service.
/// Otherwise, face authentication starts on submission, like a fingerprint reader.
pub(super) fn check_kde_login_greeter(report: &mut Report, plasmalogin_face: Option<&str>) {
    const NAME: &str = "KDE 登录界面";
    match plasmalogin_face {
        None => report.pass(
            NAME,
            "上游没有前置生物识别服务，因此在你提交登录表单时才会进行人脸认证（在密码框为空时按回车）",
        ),
        Some(contents) if slot_status(Some(contents)) == KdeLockStatus::Wired => report.pass(
            NAME,
            "plasmalogin-fingerprint 会调用 Gaze，登录界面一显示你的用户就开始人脸认证",
        ),
        Some(_) => report.warning(
            NAME,
            format!("{PLASMALOGIN_FACE_PAM_FILE} exists but does not run Gaze, so face authentication at the greeter waits for you to submit the form"),
            "请运行 `sudo gaze-kde-pam enable-login`，以便在你输入前完成扫描。",
        ),
    }
}

/// KDE slots and login forms cannot answer a confirmation prompt.
/// With confirmation required, PAM ignores Gaze and leaves password authentication available.
pub(super) fn check_kde_confirmation(
    report: &mut Report,
    config: Option<&Config>,
    kde_fingerprint: Option<&str>,
    kde_smartcard: Option<&str>,
    plasmalogin_face: Option<&str>,
    login_greeters: &[&str],
) {
    const NAME: &str = "KDE 确认提示";
    let Some(config) = config else {
        return;
    };
    if !config.auth.require_confirmation_lock_screen {
        return;
    }
    let mut blocked = Vec::new();
    if slot_status(kde_fingerprint) == KdeLockStatus::Wired {
        blocked.push(KDE_FACE_PAM_FILE.trim_start_matches("/etc/pam.d/"));
    }
    if slot_status(kde_smartcard) == KdeLockStatus::Wired {
        blocked.push(KDE_SMARTCARD_PAM_FILE.trim_start_matches("/etc/pam.d/"));
    }
    if plasmalogin_face.is_some_and(|contents| slot_status(Some(contents)) == KdeLockStatus::Wired)
    {
        blocked.push(PLASMALOGIN_FACE_PAM_FILE.trim_start_matches("/etc/pam.d/"));
    }
    blocked.extend_from_slice(login_greeters);
    if blocked.is_empty() {
        return;
    }
    report.warning(
        NAME,
        format!(
            "require_confirmation_lock_screen is on, but {} cannot be prompted, so Gaze returns PAM_IGNORE and requires password fallback there",
            blocked.join(", ")
        ),
        "请使用密码，或改用能够呈现确认的途径（带 TTY 的 sudo、polkit、GNOME）。只有在你确实需要无确认的人脸认证时才关闭该开关。详见 KDE 指南。",
    );
}

pub(super) fn hyprlock_selects_gaze(contents: &str) -> bool {
    contents.lines().any(|line| {
        let line = line.split('#').next().unwrap_or_default();
        let Some((key, value)) = line.split_once('=') else {
            return false;
        };
        matches!(key.trim(), "module" | "pam_module") && value.trim().starts_with("hyprlock-gaze")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hyprlock_modern_pam_module_key_is_detected() {
        let contents = "auth {\n    pam {\n        module = hyprlock-gaze\n    }\n}\n";
        assert!(hyprlock_selects_gaze(contents));
    }

    #[test]
    fn hyprlock_legacy_pam_module_key_is_detected() {
        let contents = "general {\n    pam_module = hyprlock-gaze-simultaneous\n}\n";
        assert!(hyprlock_selects_gaze(contents));
    }

    #[test]
    fn hyprlock_without_gaze_is_not_detected() {
        let contents = "auth {\n    pam {\n        module = hyprlock\n    }\n}\n";
        assert!(!hyprlock_selects_gaze(contents));
    }

    #[test]
    fn hyprlock_commented_out_module_is_not_detected() {
        let contents = "auth {\n    pam {\n        # module = hyprlock-gaze\n    }\n}\n";
        assert!(!hyprlock_selects_gaze(contents));
    }

    #[test]
    fn slot_status_reads_the_auth_stack() {
        assert_eq!(
            slot_status(Some(
                "#%PAM-1.0\nauth        [success=done default=ignore]                pam_gaze.so"
            )),
            KdeLockStatus::Wired
        );
        assert_eq!(
            slot_status(Some(
                "auth required pam_fprintd.so\nauth sufficient pam_gaze.so"
            )),
            KdeLockStatus::Wired
        );
        assert_eq!(
            slot_status(Some("auth sufficient pam_gaze.so simultaneous")),
            KdeLockStatus::Grosshack
        );
        assert_eq!(
            slot_status(Some("auth sufficient pam_gaze_grosshack.so")),
            KdeLockStatus::Grosshack
        );
        assert_eq!(
            slot_status(Some(
                "auth required pam_fprintd.so\nauth required pam_deny.so"
            )),
            KdeLockStatus::NotWired
        );
        assert_eq!(
            slot_status(Some("# auth sufficient pam_gaze.so")),
            KdeLockStatus::NotWired
        );
        assert_eq!(
            slot_status(Some("session optional pam_gaze.so")),
            KdeLockStatus::NotWired
        );
        assert_eq!(slot_status(None), KdeLockStatus::NoService);

        // What gaze-kde-pam actually writes: `-` so a missing module is skipped.
        assert_eq!(
            slot_status(Some(
                "-auth       [success=done default=ignore]                pam_gaze.so"
            )),
            KdeLockStatus::Wired,
            "the reported state must match the line the helper installs"
        );
    }

    #[test]
    fn either_biometric_slot_counts_as_wired() {
        let reader = Some("auth required pam_fprintd.so");
        let gaze = Some("auth [success=done default=ignore] pam_gaze.so");

        assert_eq!(
            kde_lock_status(gaze, reader),
            (KdeLockStatus::Wired, KDE_FACE_PAM_FILE)
        );
        assert_eq!(
            kde_lock_status(reader, gaze),
            (KdeLockStatus::Wired, KDE_SMARTCARD_PAM_FILE),
            "the smartcard slot is a first-class home for Gaze"
        );
        assert_eq!(
            kde_lock_status(reader, None),
            (KdeLockStatus::NotWired, KDE_FACE_PAM_FILE)
        );
        assert_eq!(
            kde_lock_status(None, None),
            (KdeLockStatus::NoService, KDE_FACE_PAM_FILE)
        );
        assert_eq!(
            kde_lock_status(Some("auth sufficient pam_gaze_grosshack.so"), reader),
            (KdeLockStatus::Grosshack, KDE_FACE_PAM_FILE),
            "a deadlocking module must be reported over a merely unwired slot"
        );
    }

    #[test]
    fn kde_lock_screen_check_warns_unless_the_plain_module_is_wired() {
        let level = |fingerprint: Option<&str>, smartcard: Option<&str>| {
            let mut report = Report::default();
            check_kde_lock_screen(&mut report, fingerprint, smartcard, true);
            report
                .checks
                .iter()
                .find(|check| check.name == "KDE 锁屏")
                .map(|check| check.level)
                .expect("the KDE lock screen check always reports")
        };

        assert_eq!(
            level(Some("auth sufficient pam_gaze.so"), None),
            Level::Pass
        );
        assert_eq!(
            level(
                Some("auth required pam_fprintd.so"),
                Some("auth sufficient pam_gaze.so")
            ),
            Level::Pass
        );
        assert_eq!(
            level(Some("auth sufficient pam_gaze.so simultaneous"), None),
            Level::Warning
        );
        assert_eq!(
            level(Some("auth sufficient pam_gaze_grosshack.so"), None),
            Level::Warning
        );
        assert_eq!(
            level(Some("auth required pam_fprintd.so"), None),
            Level::Warning
        );
        assert_eq!(level(None, None), Level::Warning);
    }

    #[test]
    fn plasma_68_never_counts_the_smartcard_slot_as_wired() {
        let check = |fingerprint: Option<&str>, smartcard: Option<&str>| {
            let mut report = Report::default();
            check_kde_lock_screen(&mut report, fingerprint, smartcard, false);
            report
                .checks
                .into_iter()
                .find(|check| check.name == "KDE 锁屏")
                .map(|check| (check.level, check.message))
                .expect("the KDE lock screen check always reports")
        };
        let reader = Some("auth required pam_fprintd.so");
        let gaze = Some("auth [success=done default=ignore] pam_gaze.so");

        let (level, message) = check(reader, gaze);
        assert_eq!(level, Level::Warning);
        assert!(message.contains("kde-smartcard"), "{message}");
        assert!(message.contains("6.8"), "{message}");

        assert_eq!(check(gaze, gaze).0, Level::Pass);
        assert_eq!(check(gaze, None).0, Level::Pass);
        let (level, message) = check(reader, None);
        assert_eq!(level, Level::Warning);
        assert!(message.contains("kde-fingerprint"), "{message}");
    }

    #[test]
    fn kde_confirmation_fallback_is_reported_when_the_toggle_is_on_and_a_slot_is_wired() {
        let check = |confirmation: bool,
                     fingerprint: Option<&str>,
                     smartcard: Option<&str>,
                     face: Option<&str>,
                     greeters: &[&str]| {
            let mut config = Config::default();
            config.auth.require_confirmation_lock_screen = confirmation;
            let mut report = Report::default();
            check_kde_confirmation(
                &mut report,
                Some(&config),
                fingerprint,
                smartcard,
                face,
                greeters,
            );
            report
                .checks
                .iter()
                .find(|check| check.name == "KDE 确认提示")
                .map(|check| (check.level, check.message.clone()))
        };

        // Off means nothing to say, even when a slot is wired.
        assert!(check(false, Some("auth sufficient pam_gaze.so"), None, None, &[]).is_none());
        assert!(check(false, None, None, None, &["plasmalogin"]).is_none());
        // On with nothing wired means there is no affected service.
        assert!(check(true, None, None, None, &[]).is_none());
        assert!(check(true, Some("auth required pam_fprintd.so"), None, None, &[]).is_none());

        let (level, message) = check(true, Some("auth sufficient pam_gaze.so"), None, None, &[])
            .expect("a wired slot with confirmation on must warn");
        assert_eq!(level, Level::Warning);
        assert!(message.contains("kde-fingerprint"), "{message}");
        assert!(message.contains("PAM_IGNORE"), "{message}");
        assert!(message.contains("password fallback"), "{message}");
        assert!(
            message.contains("require_confirmation_lock_screen"),
            "{message}"
        );

        let (_, message) = check(
            true,
            None,
            Some("auth sufficient pam_gaze.so"),
            Some("auth sufficient pam_gaze.so"),
            &[],
        )
        .expect("both smartcard and greeter slots must warn");
        assert!(message.contains("kde-smartcard"), "{message}");
        assert!(message.contains("plasmalogin-fingerprint"), "{message}");

        let (_, message) = check(true, None, None, None, &["sddm", "plasmalogin"])
            .expect("a login greeter reaching Gaze must warn");
        assert!(message.contains("sddm, plasmalogin"), "{message}");
    }

    #[test]
    fn login_greeter_check_only_complains_about_an_unused_slot() {
        let level = |contents: Option<&str>| {
            let mut report = Report::default();
            check_kde_login_greeter(&mut report, contents);
            report
                .checks
                .iter()
                .find(|check| check.name == "KDE 登录界面")
                .map(|check| check.level)
                .expect("the KDE login greeter check always reports")
        };

        // Nothing to wire is the normal state today, not a problem to fix.
        assert_eq!(level(None), Level::Pass);
        assert_eq!(level(Some("auth sufficient pam_gaze.so")), Level::Pass);
        assert_eq!(level(Some("auth required pam_fprintd.so")), Level::Warning);
    }
}
