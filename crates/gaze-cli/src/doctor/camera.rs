// SPDX-FileCopyrightText: 2026 Gundu Labs
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

pub(super) fn detected_source_remedy(
    cameras: &[(String, String)],
    key: &str,
    automatic: Option<&str>,
) -> String {
    let detected: Vec<&str> = cameras
        .iter()
        .filter(|(_, target)| target != gaze_core::config::DEFAULT_RGB_CAMERA)
        .map(|(_, target)| target.as_str())
        .collect();

    if detected.is_empty() {
        return match automatic {
            Some(automatic) => format!(
                "No PipeWire source is currently advertised. Reconnect the camera, then set \
                 {key} to a detected source, or to \"{automatic}\" to resolve it at runtime."
            ),
            None => format!(
                "No PipeWire source is currently advertised. Reconnect the camera, then set \
                 {key} to a detected source."
            ),
        };
    }

    format!(
        "Run `gaze config` to pick one interactively, or set {key} to one of the detected \
         sources: {}",
        detected
            .iter()
            .map(|target| format!("\"{target}\""))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

pub(super) fn gstreamer_package_hint_for(os_release: &str) -> &'static str {
    let os_release = os_release.to_ascii_lowercase();
    if ["arch", "manjaro", "omarchy"]
        .iter()
        .any(|family| os_release.contains(family))
    {
        "可用 `sudo pacman -S gst-plugins-base gst-plugins-good gst-plugin-pipewire` 安装。"
    } else if ["debian", "ubuntu"]
        .iter()
        .any(|family| os_release.contains(family))
    {
        "可用 `sudo apt install gstreamer1.0-plugins-base gstreamer1.0-plugins-good gstreamer1.0-pipewire` 安装。"
    } else if ["fedora", "rhel", "centos"]
        .iter()
        .any(|family| os_release.contains(family))
    {
        "可用 `sudo dnf install gstreamer1-plugins-base gstreamer1-plugins-good pipewire-gstreamer` 安装。"
    } else if os_release.contains("suse") {
        "可用 `sudo zypper install gstreamer-plugins-base gstreamer-plugins-good gstreamer-plugin-pipewire` 安装。"
    } else {
        "请安装本发行版的 GStreamer base、good 与 PipeWire 插件包。"
    }
}

pub(super) fn gstreamer_package_hint() -> &'static str {
    let os_release = fs::read_to_string("/etc/os-release").unwrap_or_default();
    gstreamer_package_hint_for(&os_release)
}

pub(super) fn check_gstreamer_plugins(report: &mut Report) -> bool {
    match gaze_vision::camera::missing_camera_elements() {
        Ok(missing) if missing.is_empty() => {
            report.pass(
                "GStreamer 插件",
                "base、JPEG/V4L2 与 PipeWire 摄像头元件均可用",
            );
            true
        }
        Ok(missing) => {
            report.error(
                "GStreamer 插件",
                format!("缺少必需的摄像头元件：{}", missing.join(", ")),
                gstreamer_package_hint(),
            );
            false
        }
        Err(err) => {
            report.error(
                "GStreamer 插件",
                format!("无法初始化插件注册表：{err}"),
                gstreamer_package_hint(),
            );
            false
        }
    }
}

pub(super) fn check_cameras(report: &mut Report, config: Option<&Config>, config_from_file: bool) {
    if !check_gstreamer_plugins(report) {
        return;
    }

    let Some(config) = config else {
        return;
    };

    let rgb = config.cameras.rgb.first();
    if matches!(
        config.cameras.rgb,
        gaze_core::config::CameraSources::Ordered(_)
    ) && !rgb.is_empty()
    {
        report.pass(
            "RGB 摄像头",
            format!(
                "运行时将按顺序尝试来源 {}；不可用的来源会被跳过",
                config.cameras.rgb
            ),
        );
    } else if !rgb.is_empty() {
        match gaze_vision::camera::enumerate_cameras() {
            Ok(cameras) => {
                let detected = cameras
                    .iter()
                    .filter(|(_, target)| target != gaze_core::config::DEFAULT_RGB_CAMERA)
                    .count();
                if rgb == gaze_core::config::DEFAULT_RGB_CAMERA {
                    if detected > 0 {
                        report.pass(
                            "RGB 摄像头",
                            format!("通过 PipeWire 可见 {detected} 个彩色摄像头"),
                        );
                    } else {
                        report.warning(
                            "RGB 摄像头",
                            "PipeWire 未发现任何物理彩色摄像头",
                            "请检查摄像头隐私开关，并在本地桌面会话中运行 `gaze config`。",
                        );
                    }
                } else if rgb.starts_with("pipewiresrc target-object=") {
                    if cameras.iter().any(|(_, target)| target == rgb) {
                        report.pass("RGB 摄像头", "已配置的 PipeWire 来源可见");
                    } else {
                        report.error(
                            "RGB 摄像头",
                            format!("已配置的来源不可见：{rgb}"),
                            detected_source_remedy(
                                &cameras,
                                "cameras.rgb",
                                Some(gaze_core::config::DEFAULT_RGB_CAMERA),
                            ),
                        );
                    }
                } else if let Some((vid, pid)) = gaze_vision::camera::parse_usb_spec(rgb) {
                    report.pass(
                        "RGB 摄像头",
                        format!("运行时解析 USB {vid:04x}:{pid:04x} 的彩色设备节点"),
                    );
                } else if let Some(name) = gaze_vision::camera::parse_libcamera_spec(rgb) {
                    check_libcamera_camera(report, "RGB 摄像头", "cameras.rgb", name);
                } else if rgb.starts_with("/dev/video") {
                    match fs::metadata(rgb) {
                        Ok(metadata) if metadata.file_type().is_char_device() => {
                            report.pass("RGB 摄像头", format!("{rgb} 是字符设备"));
                        }
                        Ok(_) => report.error(
                            "RGB 摄像头",
                            format!("{rgb} 不是字符设备"),
                            "请将 cameras.rgb 指向某个 /dev/video* 节点。",
                        ),
                        Err(err) => report.error(
                            "RGB 摄像头",
                            format!("{rgb} 无法访问：{err}"),
                            "请检查设备路径与权限。",
                        ),
                    }
                } else {
                    report.warning(
                        "RGB 摄像头",
                        "已配置自定义 GStreamer 来源，本次只读检查不会打开它",
                        "运行 `gaze test` 验证该自定义来源能否输出画面。",
                    );
                }
            }
            Err(err) => report.error(
                "RGB 摄像头",
                format!("GStreamer 摄像头枚举失败：{err}"),
                "请确认已安装 GStreamer PipeWire 插件且 PipeWire 正在运行。",
            ),
        }
    }

    let ir = config.cameras.ir.trim();
    if ir.is_empty() {
        return;
    }
    if ir.starts_with("/dev/video") {
        match fs::metadata(ir) {
            Ok(metadata) if metadata.file_type().is_char_device() => {
                check_ir_node(report, &config.cameras, ir, config_from_file);
            }
            Ok(_) => report.error(
                "红外摄像头",
                format!("{ir} 不是设备节点"),
                "请选择红外摄像头对应的 /dev/video* 节点。",
            ),
            Err(err) => report.error(
                "红外摄像头",
                format!("无法访问 {ir}：{err}"),
                "请修正 cameras.ir 或重新连接红外摄像头。",
            ),
        }
    } else if ir.starts_with("pipewiresrc target-object=") {
        match gaze_vision::camera::enumerate_ir_cameras() {
            Ok(cameras) if cameras.iter().any(|(_, target)| target == ir) => {
                report.pass("红外摄像头", "已配置的 PipeWire 来源可见");
            }
            Ok(cameras) => report.error(
                "红外摄像头",
                format!("已配置的来源不可见：{ir}"),
                detected_source_remedy(&cameras, "cameras.ir", None),
            ),
            Err(err) => report.error(
                "红外摄像头",
                format!("GStreamer 红外摄像头枚举失败：{err}"),
                "请确认 PipeWire 正在运行且红外设备已连接。",
            ),
        }
    } else if let Some((vid, pid)) = gaze_vision::camera::parse_usb_spec(ir) {
        report.pass(
            "红外摄像头",
            format!("运行时解析 USB {vid:04x}:{pid:04x} 的红外设备节点"),
        );
    } else if let Some(name) = gaze_vision::camera::parse_libcamera_spec(ir) {
        check_libcamera_camera(report, "红外摄像头", "cameras.ir", name);
    } else {
        report.warning(
            "红外摄像头",
            "已配置自定义 GStreamer 来源，本次只读检查不会打开它",
            "运行 `gaze test` 验证红外来源能否输出画面。",
        );
    }

    if config.cameras.emitter_enabled {
        check_i2c_emitter(report, ir);
    }
}

fn check_ir_node(
    report: &mut Report,
    cameras: &gaze_core::config::CameraConfig,
    node: &str,
    config_from_file: bool,
) {
    if gaze_vision::camera::is_physical_node(node) {
        report.pass("红外摄像头", format!("{node} 是字符设备"));
        return;
    }
    if !config_from_file {
        // The daemon's D-Bus config cannot carry the file-only provenance keys, so their
        // absence proves nothing when the config file itself was unreadable.
        report.warning(
            "红外摄像头",
            format!("{node} 是虚拟设备，无法验证其桥接来源"),
            "The configuration file could not be read; run `sudo gaze doctor` to check \
             cameras.ir_source_marker.",
        );
        return;
    }
    if let Err(err) = cameras.validate_ir_bridge_source() {
        report.error(
            "红外摄像头",
            format!("{node} 是虚拟设备，其桥接来源无效：{err:#}"),
            "请修正配置文件中的 cameras.ir_source_marker 与 cameras.ir_source_driver。",
        );
        return;
    }
    let Some(bridge) = cameras.ir_bridge_source() else {
        report.error(
            "红外摄像头",
            format!("{node} 是虚拟摄像头，认证流程不会打开它"),
            "If a privileged userspace bridge feeds this node, set cameras.ir_source_marker to \
             the root-owned marker file naming its physical /dev/video* source (optionally \
             pinning it with cameras.ir_source_driver), or point cameras.ir at the physical \
             node.",
        );
        return;
    };
    if let Err(err) = fs::File::open(bridge.marker)
        && err.kind() == std::io::ErrorKind::PermissionDenied
    {
        // A root-owned 0600 marker satisfies the trust rules but cannot be
        // verified by an unprivileged doctor.
        report.warning(
            "红外摄像头",
            format!("当前用户无法读取桥接标记文件 {}", bridge.marker),
            "运行 `sudo gaze doctor` 验证桥接来源。",
        );
        return;
    }
    match gaze_vision::camera::check_bridge_marker(node, &bridge) {
        Ok(_) if gaze_vision::camera::node_grants_untrusted_write(node) => report.error(
            "红外摄像头",
            format!("{node} 可被非特权用户写入，认证流程会拒绝使用"),
            "Recorded footage could be replayed into the bridge node. Restrict its \
             permissions (drop the uaccess tag, root:video 0660).",
        ),
        Ok(source) => report.pass(
            "红外摄像头",
            format!("桥接标记 {} 指向物理来源 {source}", bridge.marker),
        ),
        Err(err) => report.error(
            "红外摄像头",
            format!("{node} 的桥接来源校验失败：{err:#}"),
            "请确认桥接服务正在运行，并已写入配置的标记文件。",
        ),
    }
}

fn check_libcamera_camera(report: &mut Report, check: &'static str, key: &str, name: &str) {
    if gaze_vision::camera::libcamera_camera_names()
        .iter()
        .any(|camera| camera == name)
    {
        report.pass(check, format!("libcamera 列出了 {name}"));
    } else {
        report.error(
            check,
            format!("libcamera 未列出名为 {name:?} 的摄像头"),
            format!(
                "Close other programs using the camera, install the libcamera GStreamer plugin, \
                 or set {key} to a name from `cam --list`."
            ),
        );
    }
}

pub(super) fn check_i2c_emitter(report: &mut Report, ir: &str) {
    let Some(node) = gaze_vision::camera::resolve_node(ir) else {
        return;
    };
    match gaze_core::ir::i2c::I2cEmitter::diagnose(&node) {
        None => {}
        Some(Ok(emitter)) => report.pass(
            "红外补光",
            format!("{} 在 {} 上匹配 {node}", emitter.name(), emitter.bus()),
        ),
        Some(Err(reason)) => report.warning(
            "红外补光",
            format!("{node} 的 I2C 补光配置不适用：{reason}"),
            "Load the i2c-dev module, make sure the IR bridge is running, and check that the \
             sensor driver is bound. Authentication continues without illumination until then.",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gstreamer_plugin_remedies_use_distro_package_names() {
        for (os_release, packages) in [
            (
                "ID=omarchy\nID_LIKE=arch\n",
                [
                    "gst-plugins-base",
                    "gst-plugins-good",
                    "gst-plugin-pipewire",
                ],
            ),
            (
                "ID=ubuntu\nID_LIKE=debian\n",
                [
                    "gstreamer1.0-plugins-base",
                    "gstreamer1.0-plugins-good",
                    "gstreamer1.0-pipewire",
                ],
            ),
            (
                "ID=fedora\n",
                [
                    "gstreamer1-plugins-base",
                    "gstreamer1-plugins-good",
                    "pipewire-gstreamer",
                ],
            ),
            (
                "ID=opensuse-tumbleweed\nID_LIKE=suse\n",
                [
                    "gstreamer-plugins-base",
                    "gstreamer-plugins-good",
                    "gstreamer-plugin-pipewire",
                ],
            ),
        ] {
            let hint = gstreamer_package_hint_for(os_release);
            for package in packages {
                assert!(hint.contains(package), "{hint:?} does not name {package}");
            }
        }
    }

    #[test]
    fn camera_remedy_lists_detected_sources() {
        let cameras = vec![
            (
                "Primary camera".to_string(),
                gaze_core::config::DEFAULT_RGB_CAMERA.to_string(),
            ),
            (
                "Integrated Camera".to_string(),
                "pipewiresrc target-object=v4l2_input.pci-0000_00_14_0".to_string(),
            ),
        ];

        let remedy = detected_source_remedy(
            &cameras,
            "cameras.rgb",
            Some(gaze_core::config::DEFAULT_RGB_CAMERA),
        );
        assert!(remedy.contains("cameras.rgb"), "{remedy}");
        assert!(
            remedy.contains("\"pipewiresrc target-object=v4l2_input.pci-0000_00_14_0\""),
            "the detected source must be quoted verbatim for copy-paste: {remedy}"
        );
        assert!(
            !remedy.contains("\"primary\""),
            "the primary pseudo-source is not a selectable node: {remedy}"
        );
    }

    #[test]
    fn camera_remedy_only_offers_primary_where_it_is_valid() {
        let none_detected = vec![(
            "Primary camera".to_string(),
            gaze_core::config::DEFAULT_RGB_CAMERA.to_string(),
        )];

        let rgb = detected_source_remedy(
            &none_detected,
            "cameras.rgb",
            Some(gaze_core::config::DEFAULT_RGB_CAMERA),
        );
        assert!(rgb.contains("cameras.rgb"), "{rgb}");
        assert!(rgb.contains("\"primary\""), "{rgb}");

        let ir = detected_source_remedy(&[], "cameras.ir", None);
        assert!(ir.contains("cameras.ir"), "{ir}");
        assert!(
            !ir.contains("primary"),
            "cameras.ir has no primary fallback: {ir}"
        );
    }
}
