//! Opening, editing and submitting modal dialogs (rendering lives in `dialogs`).

use super::*;

impl Selkie {
    // -----------------------------------------------------------------------
    // Modals
    // -----------------------------------------------------------------------

    pub(super) fn open_modal(&mut self, modal: Modal, window: &mut Window, cx: &mut Context<Self>) {
        self.context_menu = None;
        self.modal = Some(modal);
        // Start with the caret on rather than waiting for the next blink tick.
        self.caret_on = true;
        self.modal_focus.focus(window);
        cx.notify();
    }

    pub(super) fn close_modal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.modal = None;
        self.search_focus.focus(window);
        cx.notify();
    }

    pub(super) fn confirm_modal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(modal) = self.modal.clone() else {
            return;
        };

        match &modal {
            Modal::PullImage { input } | Modal::TagImage { input, .. } => {
                if let Err(error) = podman::validate_image_reference(input) {
                    self.toast(ToastKind::Error, error, cx);
                    self.modal_focus.focus(window);
                    return;
                }
            }
            Modal::RenameContainer { input, .. } => {
                if let Err(error) = podman::validate_container_name(input) {
                    self.toast(ToastKind::Error, error, cx);
                    self.modal_focus.focus(window);
                    return;
                }
            }
            Modal::BuildImage { form } => {
                if let Err(error) = podman::validate_image_reference(&form.tag) {
                    self.toast(ToastKind::Error, error, cx);
                    self.modal_focus.focus(window);
                    return;
                }
                let containerfile = form.containerfile.trim();
                if !std::path::Path::new(containerfile).is_file() {
                    self.toast(
                        ToastKind::Error,
                        format!("Containerfile not found: {containerfile}"),
                        cx,
                    );
                    self.modal_focus.focus(window);
                    return;
                }
                if !form.context.trim().is_empty() {
                    if let Err(error) = podman::validate_build_context(&form.context) {
                        self.toast(ToastKind::Error, error, cx);
                        self.modal_focus.focus(window);
                        return;
                    }
                }
                for platform in &form.platforms {
                    if let Err(error) = podman::validate_platform(platform) {
                        self.toast(ToastKind::Error, error, cx);
                        self.modal_focus.focus(window);
                        return;
                    }
                }
                if let Err(error) = form.arg_tokens() {
                    self.toast(ToastKind::Error, error, cx);
                    self.modal_focus.focus(window);
                    return;
                }
                if form.validate_registries {
                    if let Err(error) = podman::validate_build_registries(containerfile) {
                        self.toast(ToastKind::Error, error, cx);
                        self.modal_focus.focus(window);
                        return;
                    }
                }
            }
            Modal::RunContainer {
                name,
                ports,
                env,
                volumes,
                secrets,
                command,
                ..
            } => {
                if !name.trim().is_empty() {
                    if let Err(error) = podman::validate_container_name(name) {
                        self.toast(ToastKind::Error, error, cx);
                        self.modal_focus.focus(window);
                        return;
                    }
                }
                for result in [
                    podman::parse_ports(ports).map(|_| ()),
                    podman::parse_env_vars(env).map(|_| ()),
                    podman::parse_volume_mounts(volumes).map(|_| ()),
                    podman::parse_command_args(command).map(|_| ()),
                    podman::parse_secret_mappings(secrets).map(|_| ()),
                ] {
                    if let Err(error) = result {
                        self.toast(ToastKind::Error, error, cx);
                        self.modal_focus.focus(window);
                        return;
                    }
                }
                // Upstream only offers existing secrets; check the names here so
                // a typo fails before Podman pulls the image.
                if let Some(missing) =
                    podman::secret_mapping_names(secrets)
                        .into_iter()
                        .find(|name| {
                            !self
                                .snapshot
                                .secrets
                                .iter()
                                .any(|secret| secret.name == *name)
                        })
                {
                    self.toast(ToastKind::Error, format!("no secret named {missing}"), cx);
                    self.modal_focus.focus(window);
                    return;
                }
            }
            Modal::PushImage { image, destination } => {
                for result in [
                    podman::validate_image_reference(image).map(|_| ()),
                    podman::validate_image_reference(destination).map(|_| ()),
                ] {
                    if let Err(error) = result {
                        self.toast(ToastKind::Error, error, cx);
                        self.modal_focus.focus(window);
                        return;
                    }
                }
            }
            Modal::RegistryLogin {
                registry,
                username,
                password,
                ..
            } => {
                if let Err(error) = podman::validate_registry(registry) {
                    self.toast(ToastKind::Error, error, cx);
                    self.modal_focus.focus(window);
                    return;
                }
                if let Err(error) = podman::validate_username(username) {
                    self.toast(ToastKind::Error, error, cx);
                    self.modal_focus.focus(window);
                    return;
                }
                if password.is_empty() {
                    self.toast(ToastKind::Error, "password cannot be empty", cx);
                    self.modal_focus.focus(window);
                    return;
                }
            }
            Modal::RegistryLogout { registry } => {
                if !registry.trim().is_empty() {
                    if let Err(error) = podman::validate_registry(registry) {
                        self.toast(ToastKind::Error, error, cx);
                        self.modal_focus.focus(window);
                        return;
                    }
                }
            }
            Modal::CreateSecret { name, data, .. } => {
                if let Err(error) = podman::validate_secret_name(name) {
                    self.toast(ToastKind::Error, error, cx);
                    self.modal_focus.focus(window);
                    return;
                }
                if data.is_empty() {
                    self.toast(ToastKind::Error, "secret data cannot be empty", cx);
                    self.modal_focus.focus(window);
                    return;
                }
            }
            Modal::CreateVolume { name, driver, .. } => {
                if let Err(error) = podman::validate_volume_name(name) {
                    self.toast(ToastKind::Error, error, cx);
                    self.modal_focus.focus(window);
                    return;
                }
                if !driver.trim().is_empty() {
                    if let Err(error) = podman::validate_driver(driver) {
                        self.toast(ToastKind::Error, error, cx);
                        self.modal_focus.focus(window);
                        return;
                    }
                }
            }
            Modal::CreateNetwork {
                name,
                driver,
                subnet,
                gateway,
                ..
            } => {
                if let Err(error) = podman::validate_network_name(name) {
                    self.toast(ToastKind::Error, error, cx);
                    self.modal_focus.focus(window);
                    return;
                }
                let optional = [
                    (
                        driver,
                        podman::validate_driver as fn(&str) -> Result<&str, String>,
                    ),
                    (subnet, podman::validate_subnet),
                    (gateway, podman::validate_gateway),
                ];
                for (value, validate) in optional {
                    if !value.trim().is_empty() {
                        if let Err(error) = validate(value) {
                            self.toast(ToastKind::Error, error, cx);
                            self.modal_focus.focus(window);
                            return;
                        }
                    }
                }
            }
            Modal::CreatePod { name, ports, .. } => {
                if let Err(error) = podman::validate_pod_name(name) {
                    self.toast(ToastKind::Error, error, cx);
                    self.modal_focus.focus(window);
                    return;
                }
                if let Err(error) = podman::parse_ports(ports) {
                    self.toast(ToastKind::Error, error, cx);
                    self.modal_focus.focus(window);
                    return;
                }
            }
            Modal::PlayKube { path } => {
                if let Err(error) = podman::validate_kube_file(path) {
                    self.toast(ToastKind::Error, error, cx);
                    self.modal_focus.focus(window);
                    return;
                }
            }
            Modal::CreateConnection { form } | Modal::EditConnection { form, .. } => {
                let identity = form.identity();
                if let Err(error) =
                    podman::connection_add_command(&form.name, &form.uri, identity.as_deref())
                {
                    self.toast(ToastKind::Error, error, cx);
                    self.modal_focus.focus(window);
                    return;
                }
                // Fast feedback only: connection_update re-checks against
                // Podman's current list before renaming.
                if let Modal::EditConnection { original, .. } = &modal {
                    let name = form.name.trim();
                    if name != original
                        && self.local.connections.iter().any(|item| item.name == name)
                    {
                        self.toast(
                            ToastKind::Error,
                            format!("a connection named {name} already exists"),
                            cx,
                        );
                        self.modal_focus.focus(window);
                        return;
                    }
                }
            }
            Modal::CreateMachine {
                name,
                cpus,
                memory,
                disk,
                ..
            } => {
                if let Err(error) = podman::parse_machine_init(name, cpus, memory, disk) {
                    self.toast(ToastKind::Error, error, cx);
                    self.modal_focus.focus(window);
                    return;
                }
            }
            _ => {}
        }

        self.modal = None;
        self.search_focus.focus(window);

        match modal {
            Modal::DeleteContainer { id, name } => {
                let label = format!("Deleting {name}…");
                let success = format!("Deleted container {name}");
                self.run_task(
                    label,
                    success,
                    AfterAction::Deselect,
                    move || podman::remove_container(&id),
                    cx,
                );
            }
            Modal::RenameContainer { id, input, .. } => {
                let name = input.trim().to_string();
                let label = format!("Renaming to {name}…");
                let success = format!("Renamed container to {name}");
                self.run_task(
                    label,
                    success,
                    AfterAction::Refresh,
                    move || podman::rename_container(&id, &name),
                    cx,
                );
            }
            Modal::DeleteImage { id, name } => {
                let label = format!("Deleting {name}…");
                let success = format!("Deleted image {name}");
                self.run_task(
                    label,
                    success,
                    AfterAction::Deselect,
                    move || podman::remove_image(&id),
                    cx,
                );
            }
            Modal::DeletePod { id, name } => {
                let label = format!("Deleting pod {name}…");
                let success = format!("Deleted pod {name}");
                self.run_task(
                    label,
                    success,
                    AfterAction::Deselect,
                    move || podman::remove_pod(&id),
                    cx,
                );
            }
            Modal::DeleteSecret { id, name } => {
                let label = format!("Deleting secret {name}…");
                let success = format!("Deleted secret {name}");
                self.run_task(
                    label,
                    success,
                    AfterAction::Deselect,
                    move || podman::remove_secret(&id),
                    cx,
                );
            }
            Modal::DeleteVolume { name } => {
                let label = format!("Deleting volume {name}…");
                let success = format!("Deleted volume {name}");
                self.run_task(
                    label,
                    success,
                    AfterAction::Deselect,
                    move || podman::remove_volume(&name),
                    cx,
                );
            }
            Modal::DeleteNetwork { name } => {
                let label = format!("Deleting network {name}…");
                let success = format!("Deleted network {name}");
                self.run_task(
                    label,
                    success,
                    AfterAction::Deselect,
                    move || podman::remove_network(&name),
                    cx,
                );
            }
            Modal::ComposeDown { project } => {
                self.start_compose(&project, ComposeAction::Down, cx);
            }
            Modal::StopMachine { name } => {
                self.machine_action(name, MachineAction::Stop, cx);
            }
            Modal::BuildImage { form } => {
                let options = podman::BuildOptions {
                    containerfile: form.containerfile.trim().to_string(),
                    tag: form.tag.trim().to_string(),
                    context: Some(form.context.trim().to_string())
                        .filter(|value| !value.is_empty()),
                    platforms: form.platforms.clone(),
                    args: form.arg_tokens().unwrap_or_default(),
                };
                match podman::build_command(&options) {
                    Ok(command) => self.start_build(options, command, cx),
                    Err(error) => self.toast(ToastKind::Error, error, cx),
                }
            }
            Modal::RunContainer {
                image,
                name,
                ports,
                env,
                volumes,
                secrets,
                command,
                ..
            } => {
                let options = podman::RunOptions {
                    image: image.clone(),
                    name: Some(name.trim().to_string()).filter(|value| !value.is_empty()),
                    ports: podman::parse_ports(&ports).unwrap_or_default(),
                    env: podman::parse_env_vars(&env).unwrap_or_default(),
                    volumes: podman::parse_volume_mounts(&volumes).unwrap_or_default(),
                    secrets: podman::parse_secret_mappings(&secrets).unwrap_or_default(),
                    command: podman::parse_command_args(&command).unwrap_or_default(),
                };
                match podman::container_run_command(&options) {
                    Ok(_) => {
                        let label = format!("Starting a container from {image}…");
                        let success = format!("Started a container from {image}");
                        self.run_task(
                            label,
                            success,
                            AfterAction::Refresh,
                            move || podman::run_container(&options),
                            cx,
                        );
                    }
                    Err(error) => self.toast(ToastKind::Error, error, cx),
                }
            }
            Modal::PushImage { image, destination } => {
                match podman::push_command(&image, &destination) {
                    Ok(command) => {
                        self.start_output(
                            OutputSpec {
                                title: format!("push · {destination}"),
                                command_line: format!("$ podman push {image} {destination}"),
                                success: format!("Pushed {image} to {destination}"),
                                failure: format!("Push failed for {image}"),
                            },
                            command,
                            None,
                            cx,
                        );
                    }
                    Err(error) => self.toast(ToastKind::Error, error, cx),
                }
            }
            Modal::RegistryLogin {
                registry,
                username,
                password,
                ..
            } => match podman::login_command(&registry, &username) {
                Ok(command) => {
                    self.start_output(
                        OutputSpec {
                            title: format!("login · {registry}"),
                            command_line: format!(
                                "$ podman login --username {username} --password-stdin {registry}"
                            ),
                            success: format!("Logged in to {registry}"),
                            failure: format!("Login failed for {registry}"),
                        },
                        command,
                        Some(password),
                        cx,
                    );
                }
                Err(error) => self.toast(ToastKind::Error, error, cx),
            },
            Modal::RegistryLogout { registry } => {
                let target = registry.trim().to_string();
                let success = if target.is_empty() {
                    "Logged out of all registries".to_string()
                } else {
                    format!("Logged out of {target}")
                };
                let target = Some(target).filter(|value| !value.is_empty());
                self.run_task(
                    "Logging out…".to_string(),
                    success,
                    AfterAction::Refresh,
                    move || podman::logout(target.as_deref()),
                    cx,
                );
            }
            Modal::PullImage { input } => {
                let reference = input.trim().to_string();
                let command = match podman::pull_command(&reference) {
                    Ok(command) => command,
                    Err(error) => {
                        self.toast(ToastKind::Error, error, cx);
                        return;
                    }
                };
                self.start_output(
                    OutputSpec {
                        title: format!("pull · {reference}"),
                        command_line: format!("$ podman pull {reference}"),
                        success: format!("Pulled {reference}"),
                        failure: format!("Pull failed for {reference}"),
                    },
                    command,
                    None,
                    cx,
                );
            }
            Modal::TagImage { id, input, .. } => {
                let target = input.trim().to_string();
                let success = format!("Tagged image as {target}");
                self.run_task(
                    format!("Tagging as {target}…"),
                    success,
                    AfterAction::Refresh,
                    move || podman::tag_image(&id, &target),
                    cx,
                );
            }
            Modal::CreateSecret { name, data, .. } => {
                let name = name.trim().to_string();
                let success = format!("Created secret {name}");
                self.run_task(
                    format!("Creating secret {name}…"),
                    success,
                    AfterAction::Refresh,
                    move || podman::create_secret(&name, &data),
                    cx,
                );
            }
            Modal::CreateVolume { name, driver, .. } => {
                let name = name.trim().to_string();
                let driver = Some(driver.trim().to_string()).filter(|value| !value.is_empty());
                let success = format!("Created volume {name}");
                self.run_task(
                    format!("Creating volume {name}…"),
                    success,
                    AfterAction::Refresh,
                    move || podman::create_volume(&name, driver.as_deref()),
                    cx,
                );
            }
            Modal::CreateNetwork {
                name,
                driver,
                subnet,
                gateway,
                ..
            } => {
                let name = name.trim().to_string();
                let optional =
                    |value: &str| Some(value.trim().to_string()).filter(|value| !value.is_empty());
                let driver = optional(&driver);
                let subnet = optional(&subnet);
                let gateway = optional(&gateway);
                let success = format!("Created network {name}");
                self.run_task(
                    format!("Creating network {name}…"),
                    success,
                    AfterAction::Refresh,
                    move || {
                        podman::create_network(
                            &name,
                            driver.as_deref(),
                            subnet.as_deref(),
                            gateway.as_deref(),
                        )
                    },
                    cx,
                );
            }
            Modal::CreatePod { name, ports, .. } => {
                let name = name.trim().to_string();
                let ports = podman::parse_ports(&ports).unwrap_or_default();
                let success = format!("Created pod {name}");
                self.run_task(
                    format!("Creating pod {name}…"),
                    success,
                    AfterAction::Refresh,
                    move || podman::create_pod(&name, &ports),
                    cx,
                );
            }
            Modal::PlayKube { path } => {
                let path = path.trim().to_string();
                match podman::kube_play_command(&path) {
                    Ok(command) => {
                        self.start_output(
                            OutputSpec {
                                title: format!("kube play · {path}"),
                                command_line: format!("$ podman kube play {path}"),
                                success: format!("Played {path}"),
                                failure: format!("kube play failed for {path}"),
                            },
                            command,
                            None,
                            cx,
                        );
                    }
                    Err(error) => self.toast(ToastKind::Error, error, cx),
                }
            }
            Modal::PruneVolumes => {
                self.run_task(
                    "Pruning volumes…".to_string(),
                    "Pruned unused volumes".to_string(),
                    AfterAction::Deselect,
                    podman::prune_volumes,
                    cx,
                );
            }
            Modal::PruneNetworks => {
                self.run_task(
                    "Pruning networks…".to_string(),
                    "Pruned unused networks".to_string(),
                    AfterAction::Deselect,
                    podman::prune_networks,
                    cx,
                );
            }
            Modal::CreateConnection { form } => {
                let name = form.trimmed_name();
                let uri = form.trimmed_uri();
                let identity = form.identity();
                self.run_task(
                    format!("Adding connection {name}…"),
                    format!("Added connection {name}"),
                    AfterAction::Refresh,
                    move || podman::connection_add(&name, &uri, identity.as_deref()),
                    cx,
                );
            }
            Modal::CreateMachine {
                name,
                cpus,
                memory,
                disk,
                ..
            } => {
                let options = match podman::parse_machine_init(&name, &cpus, &memory, &disk) {
                    Ok(options) => options,
                    Err(error) => {
                        self.toast(ToastKind::Error, error, cx);
                        return;
                    }
                };
                let label = if options.name.is_empty() {
                    "the default machine".to_string()
                } else {
                    options.name.clone()
                };
                match podman::machine_init_command(&options) {
                    Ok(command) => self.start_output(
                        OutputSpec {
                            title: format!("machine init · {label}"),
                            command_line: format!("$ {}", podman::machine_init_display(&options)),
                            success: format!("Created machine {label}"),
                            failure: format!("machine init failed for {label}"),
                        },
                        command,
                        None,
                        cx,
                    ),
                    Err(error) => self.toast(ToastKind::Error, error, cx),
                }
            }
            Modal::EditConnection { original, form } => {
                self.update_connection(
                    original,
                    form.trimmed_name(),
                    form.trimmed_uri(),
                    form.identity(),
                    cx,
                );
            }
            Modal::DeleteConnection { name } => self.remove_connection(name, cx),
            Modal::DeleteMachine { name } => {
                self.run_task(
                    format!("Deleting machine {name}…"),
                    format!("Deleted machine {name}"),
                    AfterAction::Deselect,
                    move || podman::machine_remove(&name),
                    cx,
                );
            }
        }
        cx.notify();
    }

    /// Start a build, showing the exact command in the output panel.
    fn start_build(
        &mut self,
        options: podman::BuildOptions,
        command: Command,
        cx: &mut Context<Self>,
    ) {
        let mut line = format!("$ podman build -f {}", options.containerfile);
        if options.platforms.len() > 1 {
            line.push_str(&format!(" --manifest {}", options.tag));
        } else {
            line.push_str(&format!(" -t {}", options.tag));
        }
        if !options.platforms.is_empty() {
            line.push_str(&format!(" --platform {}", options.platforms.join(",")));
        }
        for arg in &options.args {
            line.push_str(&format!(" --build-arg {arg}"));
        }
        if let Some(context) = &options.context {
            line.push_str(&format!(" {context}"));
        }
        self.start_output(
            OutputSpec {
                title: format!("build · {}", options.tag),
                command_line: line,
                success: format!("Built {}", options.tag),
                failure: format!("Build failed for {}", options.tag),
            },
            command,
            None,
            cx,
        );
    }

    pub(super) fn modal_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut build_error: Option<String> = None;
        let outcome = match &mut self.modal {
            Some(Modal::PullImage { input })
            | Some(Modal::TagImage { input, .. })
            | Some(Modal::RenameContainer { input, .. }) => edit_text(input, event, cx),
            Some(Modal::BuildImage { form }) => {
                if event.keystroke.key.as_str() == "tab" {
                    form.field = form.next_field();
                    TextOutcome::Edited
                } else if form.field == BuildField::NewPlatform {
                    // Enter in the “New platform” card adds the platform instead
                    // of submitting the whole dialog.
                    if matches!(event.keystroke.key.as_str(), "enter" | "return") {
                        if let Err(error) = form.add_custom_platform() {
                            build_error = Some(error);
                        }
                        TextOutcome::Edited
                    } else {
                        edit_text(&mut form.new_platform, event, cx)
                    }
                } else {
                    let target = match form.field {
                        BuildField::Containerfile => &mut form.containerfile,
                        BuildField::Tag => &mut form.tag,
                        BuildField::Context => &mut form.context,
                        BuildField::ArgKey(index) => match form.args.get_mut(index) {
                            Some(arg) => &mut arg.key,
                            None => return,
                        },
                        BuildField::ArgValue(index) => match form.args.get_mut(index) {
                            Some(arg) => &mut arg.value,
                            None => return,
                        },
                        BuildField::NewPlatform => &mut form.new_platform,
                    };
                    edit_text(target, event, cx)
                }
            }
            Some(Modal::RunContainer {
                field,
                name,
                ports,
                env,
                volumes,
                secrets,
                command,
                ..
            }) => {
                if event.keystroke.key.as_str() == "tab" {
                    *field = field.next();
                    TextOutcome::Edited
                } else {
                    let target = match field {
                        RunField::Name => name,
                        RunField::Ports => ports,
                        RunField::Env => env,
                        RunField::Volumes => volumes,
                        RunField::Secrets => secrets,
                        RunField::Command => command,
                    };
                    edit_text(target, event, cx)
                }
            }
            Some(Modal::PushImage { destination, .. }) => edit_text(destination, event, cx),
            Some(Modal::RegistryLogout { registry }) => edit_text(registry, event, cx),
            Some(Modal::RegistryLogin {
                field,
                registry,
                username,
                password,
            }) => {
                if event.keystroke.key.as_str() == "tab" {
                    *field = field.next();
                    TextOutcome::Edited
                } else {
                    let target = match field {
                        LoginField::Registry => registry,
                        LoginField::Username => username,
                        LoginField::Password => password,
                    };
                    edit_text(target, event, cx)
                }
            }
            Some(Modal::CreateSecret { field, name, data }) => {
                if event.keystroke.key.as_str() == "tab" {
                    *field = field.next();
                    TextOutcome::Edited
                } else {
                    let target = match field {
                        SecretField::Name => name,
                        SecretField::Data => data,
                    };
                    edit_text(target, event, cx)
                }
            }
            Some(Modal::CreateVolume {
                field,
                name,
                driver,
            }) => {
                if event.keystroke.key.as_str() == "tab" {
                    *field = field.next();
                    TextOutcome::Edited
                } else {
                    let target = match field {
                        VolumeField::Name => name,
                        VolumeField::Driver => driver,
                    };
                    edit_text(target, event, cx)
                }
            }
            Some(Modal::CreateNetwork {
                field,
                name,
                driver,
                subnet,
                gateway,
            }) => {
                if event.keystroke.key.as_str() == "tab" {
                    *field = field.next();
                    TextOutcome::Edited
                } else {
                    let target = match field {
                        NetworkField::Name => name,
                        NetworkField::Driver => driver,
                        NetworkField::Subnet => subnet,
                        NetworkField::Gateway => gateway,
                    };
                    edit_text(target, event, cx)
                }
            }
            Some(Modal::CreatePod { field, name, ports }) => {
                if event.keystroke.key.as_str() == "tab" {
                    *field = field.next();
                    TextOutcome::Edited
                } else {
                    let target = match field {
                        PodField::Name => name,
                        PodField::Ports => ports,
                    };
                    edit_text(target, event, cx)
                }
            }
            Some(Modal::CreateConnection { form } | Modal::EditConnection { form, .. }) => {
                if event.keystroke.key.as_str() == "tab" {
                    form.field = form.field.next();
                    TextOutcome::Edited
                } else {
                    edit_text(form.field_mut(), event, cx)
                }
            }
            Some(Modal::PlayKube { path }) => edit_text(path, event, cx),
            Some(Modal::CreateMachine {
                field,
                name,
                cpus,
                memory,
                disk,
            }) => {
                if event.keystroke.key.as_str() == "tab" {
                    *field = field.next();
                    TextOutcome::Edited
                } else {
                    let target = match field {
                        MachineField::Name => name,
                        MachineField::Cpus => cpus,
                        MachineField::Memory => memory,
                        MachineField::Disk => disk,
                    };
                    edit_text(target, event, cx)
                }
            }
            Some(_) => match event.keystroke.key.as_str() {
                "escape" => TextOutcome::Cancel,
                "enter" | "return" => TextOutcome::Submit,
                _ => TextOutcome::Ignored,
            },
            None => TextOutcome::Ignored,
        };

        if let Some(error) = build_error {
            self.toast(ToastKind::Error, error, cx);
        }

        match outcome {
            TextOutcome::Submit => {
                cx.stop_propagation();
                self.confirm_modal(window, cx)
            }
            TextOutcome::Cancel => {
                cx.stop_propagation();
                self.close_modal(window, cx)
            }
            TextOutcome::Edited => {
                self.caret_on = true;
                cx.notify();
            }
            TextOutcome::Ignored => cx.notify(),
        }
    }
}
