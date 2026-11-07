use super::*;

const IMAGES_JSON: &str = r#"[
    {
        "Id": "44b0f0cf834cc630726b4c86bcfcc6bea5b1916b367775ca6386d0b6678a8493",
        "RepoTags": null,
        "RepoDigests": ["localhost/app@sha256:abc"],
        "Size": 4848448920,
        "Containers": 1,
        "Digest": "sha256:a9b84cd248ad02fd338c2fee26aac5ff462b288a0663e112721f96480990144e",
        "Names": ["localhost/ladybird-deb-trixie:latest"],
        "Created": 1793907506,
        "CreatedAt": "2026-11-05T19:38:26Z"
    },
    {
        "Id": "f59c2f9df6abd26746da9c8c0c4903abbf90f519a91ae89ec94b2e5b91c97131",
        "RepoTags": ["docker.io/library/alpine:3.20"],
        "Size": 7600000,
        "Containers": 0,
        "Digest": "",
        "Names": [],
        "CreatedAt": "2026-01-02T03:04:05Z"
    }
]"#;

const CONTAINERS_JSON: &str = r#"[
    {
        "Id": "c966c39be3c11ca9ba7dff21058738cebd250890dc70d16c0176b51d06ab41fd",
        "Image": "docker.io/timescale/timescaledb:latest-pg15",
        "Names": ["spamurai-postgres-dev"],
        "State": "running",
        "Status": "Up 3 weeks",
        "CreatedAt": "3 weeks ago",
        "Ports": [
            { "host_ip": "0.0.0.0", "container_port": 5432, "host_port": 5432, "protocol": "tcp" }
        ]
    },
    {
        "Id": "66cebc9d3adb8adedc4f41b3c54ef2f4fbc39b1f6d014c252b3a4a614d8d8de7",
        "Image": "docker.io/library/nats:2.10-alpine",
        "Names": ["spamurai-nats-dev"],
        "State": "exited",
        "Status": "Exited (0) 2 days ago",
        "CreatedAt": "2 days ago",
        "Ports": []
    }
]"#;

#[test]
fn parses_images() {
    let images = parse_images(IMAGES_JSON).expect("images should parse");
    assert_eq!(images.len(), 2);
    assert_eq!(images[0].name, "localhost/ladybird-deb-trixie:latest");
    assert_eq!(images[0].id_short(), "44b0f0cf834c");
    assert_eq!(images[0].created, "2026-11-05 19:38");
    assert_eq!(images[0].size_label(), "4.8 GB");
    assert_eq!(images[1].name, "docker.io/library/alpine:3.20");
    assert_eq!(images[1].digest_short(), "");
}

#[test]
fn deduplicates_multi_tag_images() {
    // Podman emits one JSON entry per repo tag for an image with several names.
    let json = r#"[
        {"Id":"abc","Names":["quay.io/a/app:1","docker.io/library/app:1"],"Size":1},
        {"Id":"abc","Names":["quay.io/a/app:1","docker.io/library/app:1"],"Size":1}
    ]"#;
    let images = parse_images(json).expect("images should parse");
    assert_eq!(images.len(), 1);
    assert_eq!(images[0].name, "quay.io/a/app:1");
}

#[test]
fn parses_containers() {
    let containers = parse_containers(CONTAINERS_JSON).expect("containers should parse");
    assert_eq!(containers.len(), 2);
    assert_eq!(containers[0].name, "spamurai-postgres-dev");
    assert_eq!(containers[0].state, "running");
    assert_eq!(containers[0].ports, "5432->5432/tcp");
    assert_eq!(containers[0].status, "Up 3 weeks");
    assert_eq!(containers[1].ports, "-");
    assert_eq!(containers[1].id_short(), "66cebc9d3adb");
}

#[test]
fn builds_build_commands() {
    let command = build_command(&BuildOptions {
        containerfile: "/tmp/app/Containerfile".to_string(),
        tag: "localhost/app:latest".to_string(),
        ..Default::default()
    })
    .expect("build command");
    assert_eq!(command.get_program(), "podman");
    let args: Vec<String> = command
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        args,
        [
            "build",
            "-f",
            "/tmp/app/Containerfile",
            "-t",
            "localhost/app:latest",
            "/tmp/app"
        ]
    );

    // A bare file name uses the current directory as context.
    let command = build_command(&BuildOptions {
        containerfile: "Containerfile".to_string(),
        tag: "app".to_string(),
        ..Default::default()
    })
    .expect("relative build command");
    let args: Vec<String> = command
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    assert_eq!(args, ["build", "-f", "Containerfile", "-t", "app", "."]);

    // Context, platform and build args are passed through in order.
    let command = build_command(&BuildOptions {
        containerfile: "Containerfile".to_string(),
        tag: "app".to_string(),
        context: Some("/tmp".to_string()),
        platform: Some("linux/arm64".to_string()),
        args: vec!["A=1".to_string(), "B".to_string()],
    })
    .expect("full build command");
    let args: Vec<String> = command
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        args,
        [
            "build",
            "-f",
            "Containerfile",
            "-t",
            "app",
            "--platform",
            "linux/arm64",
            "--build-arg",
            "A=1",
            "--build-arg",
            "B",
            "/tmp"
        ]
    );

    assert!(build_command(&BuildOptions::default()).is_err());
    assert!(build_command(&BuildOptions {
        containerfile: "Containerfile".to_string(),
        tag: "bad tag".to_string(),
        ..Default::default()
    })
    .is_err());
    assert!(build_command(&BuildOptions {
        containerfile: "Containerfile".to_string(),
        tag: "app".to_string(),
        context: Some("/definitely/not/here".to_string()),
        ..Default::default()
    })
    .is_err());
}

#[test]
fn validates_platforms_and_build_args() {
    assert_eq!(validate_platform(" linux/amd64 "), Ok("linux/amd64"));
    assert_eq!(validate_platform("linux/arm64/v8"), Ok("linux/arm64/v8"));
    assert!(validate_platform("").is_err());
    assert!(validate_platform("amd64").is_err());
    assert!(validate_platform("-x/y").is_err());
    assert!(validate_platform("linux/am d64").is_err());

    assert_eq!(
        parse_build_args("A=1 B=two"),
        Ok(vec!["A=1".to_string(), "B=two".to_string()])
    );
    assert_eq!(parse_build_args("  "), Ok(Vec::new()));
    assert!(parse_build_args("=1").is_err());
    assert!(parse_build_args("A=1 --flag").is_err());
    assert!(parse_build_args("bad key=1").is_ok());
}

#[test]
fn builds_run_commands() {
    let options = RunOptions {
        image: "alpine:latest".to_string(),
        name: Some("demo".to_string()),
        ports: vec!["8080:80".to_string(), "127.0.0.1:5432:5432/tcp".to_string()],
        env: vec!["A=1".to_string(), "B".to_string()],
        volumes: vec!["/tmp/demo:/data".to_string(), "named:/var/lib".to_string()],
        command: vec!["sleep".to_string(), "60".to_string()],
    };
    let command = container_run_command(&options).expect("run command");
    assert_eq!(command.get_program(), "podman");
    let args: Vec<String> = command
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        args,
        [
            "run",
            "-d",
            "--name",
            "demo",
            "-p",
            "8080:80",
            "-p",
            "127.0.0.1:5432:5432/tcp",
            "-e",
            "A=1",
            "-e",
            "B",
            "-v",
            "/tmp/demo:/data",
            "-v",
            "named:/var/lib",
            "alpine:latest",
            "sleep",
            "60"
        ]
    );

    let minimal = container_run_command(&RunOptions {
        image: "alpine".to_string(),
        ..Default::default()
    })
    .expect("minimal run command");
    let args: Vec<String> = minimal
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    assert_eq!(args, ["run", "-d", "alpine"]);

    assert!(container_run_command(&RunOptions::default()).is_err());
    assert!(container_run_command(&RunOptions {
        image: "alpine".to_string(),
        name: Some("bad name".to_string()),
        ..Default::default()
    })
    .is_err());
    assert!(container_run_command(&RunOptions {
        image: "alpine".to_string(),
        ports: vec!["80".to_string()],
        ..Default::default()
    })
    .is_err());
}

#[test]
fn execute_does_not_append_display_args() {
    // Regression guard: the error message text must never be executed.
    let mut command = Command::new("echo");
    command.arg("hello");
    let output = execute(command, "run ignored:image").expect("echo should run");
    assert_eq!(output.trim(), "hello");
}

#[test]
fn builds_push_and_registry_commands() {
    let command = push_command("docker.io/library/alpine:latest", "quay.io/user/app:latest")
        .expect("push command");
    let args: Vec<String> = command
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        args,
        [
            "push",
            "docker.io/library/alpine:latest",
            "quay.io/user/app:latest"
        ]
    );

    // Loopback registries skip TLS verification.
    let command = push_command("alpine", "localhost:5001/app:tag").expect("local push");
    let args: Vec<String> = command
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        args,
        [
            "push",
            "--tls-verify=false",
            "alpine",
            "localhost:5001/app:tag"
        ]
    );

    let command = login_command("quay.io", "user").expect("login command");
    let args: Vec<String> = command
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        args,
        ["login", "--username", "user", "--password-stdin", "quay.io"]
    );

    let command = login_command("127.0.0.1:5001", "user").expect("local login");
    let args: Vec<String> = command
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        args,
        [
            "login",
            "--username",
            "user",
            "--password-stdin",
            "--tls-verify=false",
            "127.0.0.1:5001"
        ]
    );

    let command = logout_command(Some("quay.io")).expect("logout command");
    let args: Vec<String> = command
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    assert_eq!(args, ["logout", "quay.io"]);

    let command = logout_command(None).expect("logout all");
    let args: Vec<String> = command
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    assert_eq!(args, ["logout", "--all"]);

    assert!(push_command("alpine", "").is_err());
    assert!(push_command("alpine", "-bad").is_err());
    assert!(login_command("quay.io/path", "user").is_err());
    assert!(login_command("quay.io", "").is_err());
    assert!(logout_command(Some("-x")).is_err());
}

#[test]
fn builds_create_commands() {
    let command = volume_create_command("data", Some("local")).expect("volume create");
    let args: Vec<String> = command
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    assert_eq!(args, ["volume", "create", "--driver", "local", "data"]);

    let command = volume_create_command("data", None).expect("minimal volume");
    let args: Vec<String> = command
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    assert_eq!(args, ["volume", "create", "data"]);
    assert!(volume_create_command("", None).is_err());

    let command = network_create_command(
        "net",
        Some("bridge"),
        Some("10.88.0.0/24"),
        Some("10.88.0.1"),
    )
    .expect("network create");
    let args: Vec<String> = command
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        args,
        [
            "network",
            "create",
            "--driver",
            "bridge",
            "--subnet",
            "10.88.0.0/24",
            "--gateway",
            "10.88.0.1",
            "net"
        ]
    );
    assert!(network_create_command("net", None, Some("bad"), None).is_err());
    assert!(network_create_command("net", None, None, Some("999.1.1.1")).is_err());

    let command = pod_create_command("pod", &["8080:80".to_string()]).expect("pod create");
    let args: Vec<String> = command
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    assert_eq!(args, ["pod", "create", "--name", "pod", "-p", "8080:80"]);
    assert!(pod_create_command("bad name", &[]).is_err());

    let command = kube_play_command("Cargo.toml").expect("kube play");
    let args: Vec<String> = command
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    assert_eq!(args, ["kube", "play", "Cargo.toml"]);
    assert!(kube_play_command("/definitely/not/here.yaml").is_err());
    assert!(kube_play_command("").is_err());
}

fn args_of(command: Command) -> Vec<String> {
    command
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect()
}

#[test]
fn builds_machine_commands() {
    let options = parse_machine_init("vm1", "4", "4096", "50").expect("machine options");
    assert_eq!(
        options,
        MachineInitOptions {
            name: "vm1".to_string(),
            cpus: Some(4),
            memory: Some(4096),
            disk: Some(50),
        }
    );
    assert_eq!(
        args_of(machine_init_command(&options).expect("machine init")),
        [
            "machine",
            "init",
            "--cpus",
            "4",
            "--memory",
            "4096",
            "--disk-size",
            "50",
            "vm1"
        ]
    );
    assert_eq!(
        machine_init_display(&options),
        "podman machine init --cpus 4 --memory 4096 --disk-size 50 vm1"
    );

    // Empty fields fall back to Podman's own defaults.
    let options = parse_machine_init("", "", "", "").expect("defaults");
    assert_eq!(
        args_of(machine_init_command(&options).expect("default init")),
        ["machine", "init"]
    );
    assert_eq!(machine_init_display(&options), "podman machine init");

    assert!(parse_machine_init("bad name", "", "", "").is_err());
    assert!(parse_machine_init("vm", "0", "", "").is_err());
    assert!(parse_machine_init("vm", "lots", "", "").is_err());
    assert!(parse_machine_init("vm", "", "100", "").is_err());
    assert!(parse_machine_init("vm", "", "", "0").is_err());
    assert!(machine_init_command(&MachineInitOptions {
        name: "-x".to_string(),
        ..Default::default()
    })
    .is_err());

    // Machine commands are local and validated before they reach the CLI.
    assert_eq!(
        args_of(machine_ssh_command("vm1").expect("machine ssh")),
        ["machine", "ssh", "vm1"]
    );
    assert!(machine_ssh_command("").is_err());
    assert!(machine_ssh_command("-oProxyCommand=x").is_err());
}

#[test]
fn parses_run_options() {
    assert_eq!(
        parse_ports("8080:80 127.0.0.1:5432:5432/tcp"),
        Ok(vec![
            "8080:80".to_string(),
            "127.0.0.1:5432:5432/tcp".to_string()
        ])
    );
    assert_eq!(parse_ports("  "), Ok(Vec::new()));
    assert!(parse_ports("80").is_err());
    assert!(parse_ports("-p 80:80").is_err());

    assert_eq!(
        parse_env_vars("A=1 B"),
        Ok(vec!["A=1".to_string(), "B".to_string()])
    );
    assert_eq!(parse_env_vars(""), Ok(Vec::new()));
    assert!(parse_env_vars("=1").is_err());
    assert!(parse_env_vars("-A=1").is_err());

    assert_eq!(
        parse_volume_mounts("/tmp:/data named:/var:ro"),
        Ok(vec!["/tmp:/data".to_string(), "named:/var:ro".to_string()])
    );
    assert!(parse_volume_mounts("onlysource").is_err());
    assert!(parse_volume_mounts(":/data").is_err());

    assert_eq!(
        parse_command_args("  sh -c echo  ").unwrap(),
        vec!["sh", "-c", "echo"]
    );
    assert!(parse_command_args("").unwrap().is_empty());
    assert_eq!(
        parse_command_args(r#"sh -c "echo hi && sleep 1""#).unwrap(),
        vec!["sh", "-c", "echo hi && sleep 1"]
    );
    assert_eq!(
        parse_command_args(r#"printf '%s\n' "a \"b\"" c\ d '' x"#).unwrap(),
        vec!["printf", "%s\\n", "a \"b\"", "c d", "", "x"]
    );
    assert!(parse_command_args("sh -c 'oops").is_err());
    assert!(parse_command_args(r#"echo "oops"#).is_err());
    assert!(parse_command_args("echo \\").is_err());
}

#[test]
fn parses_sizes_from_stats() {
    assert_eq!(parse_size("0B"), 0);
    assert_eq!(parse_size("9"), 9);
    assert_eq!(parse_size("556.2kB"), 556_200);
    assert_eq!(parse_size("1.712MB"), 1_712_000);
    assert_eq!(parse_size("16.38GB"), 16_380_000_000);
    assert_eq!(parse_size("1.5GiB"), 1_610_612_736);
    assert_eq!(parse_percent("0.37%"), 0.37);
    assert_eq!(parse_io("8.111GB / 40.02MB"), (8_111_000_000, 40_020_000));
}

#[test]
fn parses_stats_json() {
    let json = r#"[
        {
            "id": "abc",
            "name": "demo",
            "cpu_percent": "12.50%",
            "mem_usage": "82.31MB / 16.38GB",
            "mem_percent": "0.50%",
            "net_io": "8.111GB / 40.02MB",
            "block_io": "1MB / 2MB",
            "pids": "42"
        }
    ]"#;

    let raw: Vec<RawStats> = parse_json(json, "podman stats").expect("stats should parse");
    let sample = raw.into_iter().next().expect("one sample").into_sample();
    assert!((sample.cpu_percent - 12.5).abs() < f64::EPSILON);
    assert_eq!(sample.mem_usage, 82_310_000);
    assert_eq!(sample.mem_limit, 16_380_000_000);
    assert_eq!(sample.mem_percent, 0.5);
    assert_eq!(sample.net_in, 8_111_000_000);
    assert_eq!(sample.net_out, 40_020_000);
    assert_eq!(sample.block_in, 1_000_000);
    assert_eq!(sample.block_out, 2_000_000);
    assert_eq!(sample.pids, 42);
    assert_eq!(sample.memory_label(), "82.3 MB / 16.4 GB");
}

#[test]
fn tolerates_empty_output() {
    assert!(parse_images("").unwrap().is_empty());
    assert!(parse_containers("  \n").unwrap().is_empty());
    assert!(parse_machines("").unwrap().is_empty());
    assert!(parse_connections("").unwrap().is_empty());
}

#[test]
fn parses_machines() {
    let json = r#"[
        {
            "Name": "podman-machine-default",
            "Default": true,
            "Running": true,
            "LastUp": "2026-11-05T10:11:12.123456789Z",
            "VMType": "qemu",
            "CPUs": 4,
            "Memory": 2147483648,
            "DiskSize": 107374182400
        },
        {
            "Name": "podman-machine-2",
            "Default": false,
            "Running": false,
            "VMType": "wsl",
            "CPUs": 2,
            "Memory": 1073741824,
            "DiskSize": 21474836480
        }
    ]"#;

    let machines = parse_machines(json).expect("machines should parse");
    assert_eq!(machines.len(), 2);
    assert_eq!(machines[0].name, "podman-machine-default");
    assert!(machines[0].running);
    assert!(machines[0].default);
    assert_eq!(machines[0].vm_type, "qemu");
    assert_eq!(machines[0].cpus, 4);
    assert_eq!(machines[0].memory_label(), "2.1 GB");
    assert_eq!(machines[0].disk_label(), "107.4 GB");
    assert_eq!(machines[0].last_up, "2026-11-05 10:11");
    assert_eq!(machines[0].state_label(), "running");
    assert!(!machines[1].running);
    assert_eq!(machines[1].state_label(), "stopped");
}

#[test]
fn parses_connections() {
    let json = r#"[
        {
            "Name": "dev-box",
            "URI": "ssh://core@host:2222/run/user/1000/podman/podman.sock",
            "Identity": "/home/me/.ssh/id_ed25519",
            "Default": true
        },
        { "Name": "plain" }
    ]"#;

    let connections = parse_connections(json).expect("connections should parse");
    assert_eq!(connections.len(), 2);
    assert_eq!(connections[0].name, "dev-box");
    assert_eq!(
        connections[0].uri,
        "ssh://core@host:2222/run/user/1000/podman/podman.sock"
    );
    assert!(connections[0].default);
    assert_eq!(connections[0].identity_label(), "/home/me/.ssh/id_ed25519");
    assert_eq!(connections[1].identity_label(), "-");
}

#[test]
fn adds_connection_flag_only_when_selected() {
    let mut command = Command::new("podman");
    apply_connection(&mut command, None);
    assert_eq!(command.get_args().count(), 0);

    apply_connection(&mut command, Some("dev-box"));
    let args: Vec<String> = command
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    assert_eq!(args, ["--connection", "dev-box"]);
}

#[test]
fn parses_compose_labels_from_both_providers() {
    let docker = r#"[
        {
            "Id": "aaa",
            "Names": ["shop-app-1"],
            "Image": "img",
            "State": "running",
            "Labels": {
                "com.docker.compose.project": "shop",
                "com.docker.compose.service": "app",
                "com.docker.compose.project.config_files": "/src/shop/compose.yaml",
                "com.docker.compose.project.working_dir": "/src/shop"
            }
        },
        {
            "Id": "bbb",
            "Names": ["shop-db-1"],
            "State": "exited",
            "Labels": {
                "io.podman.compose.project": "legacy",
                "io.podman.compose.service": "db"
            }
        },
        {
            "Id": "ccc",
            "Names": ["plain"],
            "State": "running",
            "Labels": {}
        }
    ]"#;

    let containers = parse_containers(docker).expect("containers should parse");
    let shop = containers[0].compose.as_ref().expect("compose membership");
    assert_eq!(shop.project, "shop");
    assert_eq!(shop.service.as_deref(), Some("app"));
    assert_eq!(shop.config_files.as_deref(), Some("/src/shop/compose.yaml"));
    assert_eq!(shop.working_dir.as_deref(), Some("/src/shop"));

    let legacy = containers[1]
        .compose
        .as_ref()
        .expect("podman-compose labels");
    assert_eq!(legacy.project, "legacy");
    assert_eq!(legacy.service.as_deref(), Some("db"));
    assert!(containers[2].compose.is_none());
}

#[test]
fn groups_containers_into_compose_projects() {
    let json = r#"[
        { "Id": "1", "Names": ["shop-app-1"], "State": "running",
          "Labels": { "com.docker.compose.project": "shop", "com.docker.compose.service": "app",
                      "com.docker.compose.project.config_files": "/src/shop/compose.yaml,/src/shop/compose.override.yaml",
                      "com.docker.compose.project.working_dir": "/src/shop" } },
        { "Id": "2", "Names": ["shop-app-2"], "State": "exited",
          "Labels": { "com.docker.compose.project": "shop", "com.docker.compose.service": "app" } },
        { "Id": "3", "Names": ["shop-db-1"], "State": "running",
          "Labels": { "com.docker.compose.project": "shop", "com.docker.compose.service": "db",
                      "com.docker.compose.project.config_files": "/src/shop/compose.yaml" } },
        { "Id": "4", "Names": ["other-app-1"], "State": "running",
          "Labels": { "com.docker.compose.project": "other", "com.docker.compose.service": "app" } },
        { "Id": "5", "Names": ["loose"] }
    ]"#;

    let containers = parse_containers(json).expect("containers should parse");
    let projects = compose_projects(&containers);
    assert_eq!(projects.len(), 2);

    let other = &projects[0];
    assert_eq!(other.name, "other");
    assert_eq!((other.running, other.total), (1, 1));

    let shop = &projects[1];
    assert_eq!(shop.name, "shop");
    assert_eq!((shop.running, shop.total), (2, 3));
    assert_eq!(shop.working_dir.as_deref(), Some("/src/shop"));
    assert_eq!(
        shop.config_files,
        vec!["/src/shop/compose.yaml", "/src/shop/compose.override.yaml"]
    );
    assert_eq!(
        shop.services,
        vec![
            ComposeService {
                name: "app".into(),
                containers: 2,
                running: 1
            },
            ComposeService {
                name: "db".into(),
                containers: 1,
                running: 1
            }
        ]
    );
    assert_eq!(shop.status_label(), "2/3 running");
    assert_eq!(shop.config_label(), "compose.yaml");
}

#[test]
fn builds_compose_commands() {
    let project = ComposeProject {
        name: "shop".into(),
        config_files: vec!["/src/shop/compose.yaml".into()],
        working_dir: Some("/src/shop".into()),
        services: Vec::new(),
        running: 0,
        total: 0,
    };

    let command = compose_command(&project, ComposeAction::Up).expect("up command");
    assert_eq!(command.get_program(), "podman");
    assert_eq!(
        command
            .get_envs()
            .find(|(key, _)| *key == "PODMAN_COMPOSE_WARNING_LOGS")
            .and_then(|(_, value)| value),
        Some(std::ffi::OsStr::new("false"))
    );
    let args: Vec<String> = command
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        args,
        [
            "compose",
            "-f",
            "/src/shop/compose.yaml",
            "-p",
            "shop",
            "up",
            "-d"
        ]
    );

    let command = compose_command(&project, ComposeAction::Down).expect("down command");
    let args: Vec<String> = command
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        args,
        [
            "compose",
            "-f",
            "/src/shop/compose.yaml",
            "-p",
            "shop",
            "down"
        ]
    );

    for (action, verb) in [
        (ComposeAction::Restart, "restart"),
        (ComposeAction::Build, "build"),
    ] {
        let command = compose_command(&project, action).expect("compose action");
        assert_eq!(
            args_of(command),
            [
                "compose",
                "-f",
                "/src/shop/compose.yaml",
                "-p",
                "shop",
                verb
            ]
        );
    }

    // Compose logs use --tail (works with both compose providers) and can be
    // scoped to one service.
    let command = compose_logs_command(&project, None, 200).expect("project logs");
    assert_eq!(
        args_of(command),
        [
            "compose",
            "-f",
            "/src/shop/compose.yaml",
            "-p",
            "shop",
            "logs",
            "-f",
            "--tail",
            "200"
        ]
    );
    let command = compose_logs_command(&project, Some("db"), 50).expect("service logs");
    assert_eq!(args_of(command).last().map(String::as_str), Some("db"));
    assert!(compose_logs_command(&project, Some("--all"), 50).is_err());

    let without_config = ComposeProject {
        config_files: Vec::new(),
        ..project.clone()
    };
    assert!(compose_command(&without_config, ComposeAction::Up).is_ok());

    let bare = ComposeProject {
        config_files: Vec::new(),
        working_dir: None,
        ..project
    };
    assert!(compose_command(&bare, ComposeAction::Up).is_err());
}

#[test]
fn parses_dashboard_data() {
    let json = r#"{
        "host": {
            "os": "linux", "arch": "amd64", "hostname": "darkstar",
            "kernel": "7.0.12+", "cpus": 12,
            "memFree": 762638336, "memTotal": 16378802176,
            "eventLogger": "journald", "networkBackend": "netavark",
            "serviceIsRemote": false,
            "security": { "rootless": true },
            "ociRuntime": { "name": "crun", "version": "crun version 1.21\ncommit: abc" },
            "remoteSocket": { "path": "/run/user/1000/podman/podman.sock", "exists": true }
        },
        "store": {
            "graphDriverName": "overlay",
            "graphRoot": "/home/u/.local/share/containers/storage"
        },
        "version": { "Version": "5.4.2", "OsArch": "linux/amd64" }
    }"#;

    let info = parse_system_info(json).expect("system info");
    assert_eq!(info.version, "5.4.2");
    assert_eq!(info.os_arch, "linux/amd64");
    assert_eq!(info.cpus, 12);
    assert_eq!(info.memory_total, 16_378_802_176);
    assert!(info.rootless);
    assert!(!info.remote);
    assert_eq!(info.mode_label(), "rootless");
    assert_eq!(info.runtime, "crun 1.21");
    assert_eq!(info.storage_driver, "overlay");
    assert!(info.socket_present);
    assert_eq!(info.socket_path, "/run/user/1000/podman/podman.sock");
    assert!(info.memory_label().contains("free of"));

    // Missing fields fall back to empty values instead of failing.
    let bare = parse_system_info("{}").expect("bare info");
    assert_eq!(bare.os_arch, "");
    assert_eq!(bare.mode_label(), "rootful");
    assert!(parse_system_info("not json").is_err());

    assert_eq!(
        short_runtime("crun", "crun version 1.21\ncommit: x"),
        "crun 1.21"
    );
    assert_eq!(short_runtime("runc", "runc version 1.1.12"), "runc 1.1.12");
    assert_eq!(short_runtime("crun", ""), "crun");

    let disk = parse_disk_usage(
        r#"[
            {"Type":"Images","Total":53,"Active":16,"RawSize":10077943468,
             "RawReclaimable":7808901844,"Size":"10.08GB","Reclaimable":"7.809GB (77%)"},
            {"Type":"Local Volumes","Total":3,"Active":3,"RawSize":74552741,
             "RawReclaimable":0,"Size":"74.55MB","Reclaimable":"0B (0%)"}
        ]"#,
    )
    .expect("disk usage");
    assert_eq!(disk.len(), 2);
    assert_eq!(disk[0].kind, "Images");
    assert_eq!(disk[0].total, 53);
    assert_eq!(disk[0].reclaimable_percent(), 77);
    assert_eq!(disk[0].reclaimable_label(), "7.8 GB reclaimable (77%)");
    assert_eq!(disk[1].kind, "Volumes");
    assert_eq!(disk[1].reclaimable_percent(), 0);
    assert_eq!(disk[1].reclaimable_label(), "0 B reclaimable (0%)");
}

#[test]
fn parses_image_history_and_files() {
    let history = parse_image_history(
        r#"[
            {"id":"3ea06e4cb95e726ea6c6cc4377570ac4c4050d78710004cbd631ca809daf7e8e",
             "created":"2026-11-05T18:00:28.583714131Z","CreatedBy":"CMD [\"/bin/sh\"]",
             "tags":["alpine:latest"],"size":0,"comment":"buildkit.dockerfile.v0"},
            {"id":"<missing>","created":"2026-11-05T18:00:28.583714131Z",
             "CreatedBy":"ADD alpine-minirootfs-3.24.4.tar.gz /","size":8713728,"comment":""}
        ]"#,
    )
    .expect("history");
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].created, "2026-11-05 18:00");
    assert_eq!(history[0].id_label(), "3ea06e4cb95e");
    assert_eq!(history[0].size_label(), "-");
    assert_eq!(history[1].id_label(), "-");
    assert_eq!(history[1].size_label(), "8.7 MB");
    assert_eq!(
        history[1].created_by,
        "ADD alpine-minirootfs-3.24.4.tar.gz /"
    );

    let listing = "\
        drwxr-xr-x 0/0               0 2026-11-05 09:19 bin/\n\
        lrwxrwxrwx 0/0               0 2026-11-05 09:19 bin/arch -> /bin/busybox\n\
        -rwxr-xr-x 0/0          804616 2026-01-10 15:38 bin/busybox\n\
        drwxr-xr-x 0/0               0 2026-11-05 09:19 my dir/\n\
        -rw-r--r-- 0/0              12 2026-01-10 15:38 my dir/read me.txt\n\
        -rw-r--r-- 0/0               5 2026-01-10 15:38 etc/hostname\n\
        drwxr-xr-x 0/0               0 2026-11-05 09:19 ./\n\
        garbage line\n";
    let files = parse_tar_listing(listing);
    let find = |path: &str| files.iter().find(|file| file.path == path);

    assert_eq!(
        find("bin").map(|file| file.kind),
        Some(ImageFileKind::Directory)
    );
    assert_eq!(
        find("bin/arch").and_then(|file| file.target.as_deref()),
        Some("/bin/busybox")
    );
    assert_eq!(
        find("bin/busybox").map(|file| (file.kind, file.size)),
        Some((ImageFileKind::File, 804_616))
    );
    // Names with spaces survive and keep their parent.
    let spaced = find("my dir/read me.txt").expect("file with spaces");
    assert_eq!(spaced.name(), "read me.txt");
    assert_eq!(spaced.parent(), "my dir");
    // `etc` has no directory row in the archive and is synthesized.
    assert_eq!(
        find("etc").map(|file| file.kind),
        Some(ImageFileKind::Directory)
    );
    assert!(find(".").is_none());

    assert!(validate_archive_path("etc/hostname").is_ok());
    assert!(validate_archive_path("").is_err());
    assert!(validate_archive_path("-rf").is_err());
    assert!(validate_archive_path("etc/../passwd").is_err());
}

#[test]
fn parses_machine_list_with_string_sizes() {
    // `podman machine ls --format json` reports Memory/DiskSize as strings.
    let json = r#"[{
        "Name": "podman-machine-default", "Default": true,
        "Created": "2026-11-06T18:27:02.478147833Z",
        "Running": true, "Starting": false,
        "LastUp": "0001-01-01T00:00:00Z", "Stream": "default",
        "VMType": "qemu", "CPUs": 6,
        "Memory": "2147483648", "DiskSize": "107374182400",
        "Port": 42299, "RemoteUsername": "core",
        "IdentityPath": "/home/u/.local/share/containers/podman/machine/machine",
        "UserModeNetworking": true
    }]"#;

    let machines = parse_machines(json).expect("machines should parse");
    assert_eq!(machines.len(), 1);
    let machine = &machines[0];
    assert_eq!(machine.name, "podman-machine-default");
    assert!(machine.running);
    assert!(machine.default);
    assert_eq!(machine.cpus, 6);
    assert_eq!(machine.memory, 2_147_483_648);
    assert_eq!(machine.disk_size, 107_374_182_400);
    assert_eq!(machine.vm_type, "qemu");
    assert_eq!(machine.memory_label(), "2.1 GB");
    assert_eq!(machine.disk_label(), "107.4 GB");
    // The zero timestamp means Podman has no last-up time recorded.
    assert_eq!(machine.last_up, "-");

    // Numeric sizes (other Podman versions) keep working.
    let numeric =
        r#"[{"Name":"vm","Running":false,"CPUs":2,"Memory":1073741824,"DiskSize":10737418240}]"#;
    let machines = parse_machines(numeric).expect("numeric sizes");
    assert_eq!(machines[0].memory, 1_073_741_824);
    assert_eq!(machines[0].cpus, 2);
}

#[test]
fn formats_sizes() {
    assert_eq!(human_size(0), "0 B");
    assert_eq!(human_size(999), "999 B");
    assert_eq!(human_size(1_000), "1.0 kB");
    assert_eq!(human_size(1_500_000), "1.5 MB");
    assert_eq!(human_size(4_848_448_920), "4.8 GB");
}

#[test]
fn formats_ports() {
    let ports = vec![
        RawPort {
            host_ip: "127.0.0.1".into(),
            host_port: 8080,
            container_port: 80,
            protocol: "tcp".into(),
        },
        RawPort {
            host_ip: String::new(),
            host_port: 0,
            container_port: 443,
            protocol: String::new(),
        },
    ];
    assert_eq!(ports_label(&ports), "127.0.0.1:8080->80/tcp, 443/tcp");
}

#[test]
fn maps_container_actions_to_podman_verbs() {
    assert_eq!(ContainerAction::Start.verb(), "start");
    assert_eq!(ContainerAction::Stop.verb(), "stop");
    assert_eq!(ContainerAction::Restart.verb(), "restart");
    assert_eq!(ContainerAction::Pause.verb(), "pause");
    assert_eq!(ContainerAction::Unpause.verb(), "unpause");
}

#[test]
fn validates_container_names() {
    assert_eq!(validate_container_name("  web-1 "), Ok("web-1"));
    assert_eq!(validate_container_name("a.b_c"), Ok("a.b_c"));
    assert!(validate_container_name("").is_err());
    assert!(validate_container_name("-x").is_err());
    assert!(validate_container_name("two words").is_err());
    assert!(validate_container_name("with/slash").is_err());
}

#[test]
fn reads_log_timestamps() {
    assert_eq!(
        log_timestamp("2026-11-05T22:40:01.123456789Z hello"),
        Some("2026-11-05T22:40:01.123456789Z")
    );
    assert_eq!(log_timestamp("plain log line"), None);
    assert_eq!(log_timestamp(""), None);
}

#[test]
fn pretty_prints_inspect_json() {
    let pretty = pretty_json(r#"[{"Id":"abc"}]"#);
    assert!(pretty.contains("\n  {"), "{pretty}");
    assert!(pretty.contains("\"Id\": \"abc\""), "{pretty}");
    assert_eq!(pretty_json("not json"), "not json");
}

#[test]
fn maps_inspect_kinds_to_commands() {
    assert_eq!(InspectKind::Container.args("abc"), ["inspect", "abc"]);
    assert_eq!(InspectKind::Pod.args("p"), ["pod", "inspect", "p"]);
    assert_eq!(
        InspectKind::Network.args("net"),
        ["network", "inspect", "net"]
    );
}

#[test]
fn validates_image_references() {
    assert_eq!(
        validate_image_reference("  alpine:3.20 "),
        Ok("alpine:3.20")
    );
    assert!(validate_image_reference("").is_err());
    assert!(validate_image_reference("-f").is_err());
    assert!(validate_image_reference("alpine 3.20").is_err());
}

#[test]
fn parses_inspect_summary() {
    let json = r#"[
        {
            "Id": "abc123",
            "Name": "/demo",
            "ImageName": "docker.io/library/nginx:latest",
            "Created": "2026-11-05T19:38:26.123456789Z",
            "State": { "Status": "running", "Pid": 4242, "ExitCode": 0 },
            "Config": {
                "Image": "docker.io/library/nginx:latest",
                "Entrypoint": ["/docker-entrypoint.sh"],
                "Cmd": ["nginx", "-g", "daemon off;"],
                "Env": ["A=1", "B=2"]
            },
            "Mounts": [
                { "Type": "bind", "Source": "/tmp", "Destination": "/data" }
            ],
            "NetworkSettings": {
                "Networks": { "bridge": { "IPAddress": "10.0.0.2" } }
            }
        }
    ]"#;

    let details = parse_inspect(json).expect("inspect should parse");
    assert_eq!(details.name, "demo");
    assert_eq!(details.image, "docker.io/library/nginx:latest");
    assert_eq!(
        details.command,
        "/docker-entrypoint.sh nginx -g daemon off;"
    );
    assert_eq!(details.created, "2026-11-05 19:38");
    assert_eq!(details.pid, 4242);
    assert_eq!(details.env, vec!["A=1", "B=2"]);
    assert_eq!(details.mounts.len(), 1);
    assert_eq!(details.mounts[0].destination, "/data");
    assert_eq!(details.networks, vec!["bridge"]);
}

#[test]
fn reads_compose_labels_from_inspect() {
    let json = r#"[
        {
            "Id": "abc",
            "Name": "/shop-app-1",
            "Config": {
                "Labels": {
                    "com.docker.compose.project": "shop",
                    "com.docker.compose.service": "app"
                }
            }
        }
    ]"#;

    let details = parse_inspect(json).expect("inspect should parse");
    assert_eq!(details.compose_project.as_deref(), Some("shop"));
    assert_eq!(details.compose_service.as_deref(), Some("app"));
}

#[test]
fn parses_pods() {
    let json = r#"[
        {
            "Id": "2409f9b6b5ed73168a3fe68377b871ebbbfe457a0b161d8cb38d2aa01428a34d",
            "Name": "pod_jobs",
            "Created": "2025-09-21T19:58:44.455768304+01:00",
            "Status": "Running",
            "Containers": [{ "Id": "a" }, { "Id": "b" }]
        }
    ]"#;

    let pods = parse_pods(json).expect("pods should parse");
    assert_eq!(pods.len(), 1);
    assert_eq!(pods[0].name, "pod_jobs");
    assert_eq!(pods[0].status, "Running");
    assert_eq!(pods[0].containers, 2);
    assert_eq!(pods[0].id_short(), "2409f9b6b5ed");
    assert_eq!(pods[0].created, "2025-09-21 19:58");
}

#[test]
fn parses_volumes() {
    let json = r#"[
        {
            "Name": "pgdata",
            "Driver": "local",
            "Mountpoint": "/home/user/.local/share/containers/storage/volumes/pgdata/_data",
            "CreatedAt": "2025-10-26T19:20:31.258791278Z",
            "Scope": "local",
            "Anonymous": false,
            "MountCount": 3
        }
    ]"#;

    let volumes = parse_volumes(json).expect("volumes should parse");
    assert_eq!(volumes.len(), 1);
    assert_eq!(volumes[0].name, "pgdata");
    assert_eq!(volumes[0].driver, "local");
    assert_eq!(volumes[0].mounts, 3);
    assert_eq!(volumes[0].created, "2025-10-26 19:20");
}

#[test]
fn parses_networks() {
    let json = r#"[
        {
            "name": "spamurai-dev",
            "id": "e714087238e33105bd612ee3299437f683b92da88d1cc70e7e39fb28e587f1a5",
            "driver": "bridge",
            "network_interface": "podman1",
            "created": "2026-10-11T12:45:03.960554832+01:00",
            "subnets": [
                { "subnet": "10.89.0.0/24", "gateway": "10.89.0.1" }
            ],
            "ipv6_enabled": false,
            "internal": false,
            "dns_enabled": true
        }
    ]"#;

    let networks = parse_networks(json).expect("networks should parse");
    assert_eq!(networks.len(), 1);
    assert_eq!(networks[0].name, "spamurai-dev");
    assert_eq!(networks[0].subnet_label(), "10.89.0.0/24");
    assert!(networks[0].dns);
    assert_eq!(networks[0].id_short(), "e714087238e3");
}

#[test]
fn parses_network_inspect_containers() {
    let json = r#"[
        {
            "name": "spamurai-dev",
            "id": "e714087238e33105bd612ee3299437f683b92da88d1cc70e7e39fb28e587f1a5",
            "driver": "bridge",
            "network_interface": "podman1",
            "created": "2026-10-11T12:45:03Z",
            "subnets": [{ "subnet": "10.89.0.0/24", "gateway": "10.89.0.1" }],
            "containers": {
                "66cebc9d3adb8adedc4f41b3c54ef2f4fbc39b1f6d014c252b3a4a614d8d8de7": { "name": "nats" }
            }
        }
    ]"#;

    let details = parse_network_inspect(json).expect("network inspect should parse");
    assert_eq!(details.containers, vec!["nats"]);
    assert_eq!(details.subnets[0].gateway, "10.89.0.1");
}

#[test]
fn maps_pod_actions_to_podman_verbs() {
    assert_eq!(PodAction::Start.verb(), "start");
    assert_eq!(PodAction::Stop.verb(), "stop");
    assert_eq!(PodAction::Restart.verb(), "restart");
}

#[test]
fn decodes_stored_credentials() {
    assert_eq!(base64_decode("dXNlcjpwYXNz"), Some(b"user:pass".to_vec()));
    assert_eq!(base64_decode("YWJj"), Some(b"abc".to_vec()));
    assert_eq!(base64_decode("YWJj\n"), Some(b"abc".to_vec()));
    assert_eq!(base64_decode("not base64!"), None);
}

#[test]
fn parses_registry_accounts_from_an_auth_file() {
    let json = r#"{"auths":{
        "quay.io": {"auth": "dXNlcjpwYXNz"},
        "registry.example.com": {"identitytoken": "abc"},
        "empty.example.com": {}
    }}"#;
    let accounts = parse_accounts(json);
    assert_eq!(accounts.len(), 3);

    let quay = accounts
        .iter()
        .find(|account| account.registry == "quay.io")
        .expect("quay.io");
    assert_eq!(quay.username, "user");
    assert_eq!(quay.kind, AccountKind::Password);

    let token = accounts
        .iter()
        .find(|account| account.registry == "registry.example.com")
        .expect("token registry");
    assert_eq!(token.kind, AccountKind::Token);
    assert_eq!(token.username, "unknown");
}

#[test]
fn ignores_malformed_auth_files() {
    assert!(parse_accounts("not json").is_empty());
    assert!(parse_accounts("{}").is_empty());
    assert!(parse_accounts(r#"{"auths":{}}"#).is_empty());
}

#[test]
fn lists_registries_backed_by_credential_helpers() {
    let json = r#"{
        "auths": {"ghcr.io": {}, "quay.io": {"auth": "dXNlcjpwYXNz"}},
        "credsStore": "desktop",
        "credHelpers": {"gcr.io": "gcloud", "quay.io": "ecr-login"}
    }"#;
    let accounts = parse_accounts(json);
    let find = |registry: &str| {
        accounts
            .iter()
            .find(|account| account.registry == registry)
            .unwrap_or_else(|| panic!("missing {registry}"))
    };
    assert_eq!(find("ghcr.io").kind, AccountKind::Helper);
    assert_eq!(find("gcr.io").kind, AccountKind::Helper);
    assert_eq!(find("gcr.io").username, "unknown");
    assert_eq!(find("quay.io").kind, AccountKind::Password);
    assert_eq!(find("quay.io").username, "user");
    assert_eq!(accounts.len(), 3);
}

#[test]
fn loopback_registry_detection_is_exact() {
    for local in [
        "localhost",
        "localhost:5000",
        "127.0.0.1:5001/app:tag",
        "[::1]",
        "[::1]:5000",
        "docker://localhost:5000/app",
    ] {
        assert!(is_loopback_registry(local), "{local}");
    }
    for remote in [
        "[fe80::1]:5000",
        "[2001:db8::1]",
        "[::10]",
        "[::1]x",
        "localhost.example.com",
        "quay.io",
    ] {
        assert!(!is_loopback_registry(remote), "{remote}");
    }
}

#[test]
fn queries_are_killed_after_the_timeout() {
    let mut command = Command::new("sleep");
    command.arg("10");
    let started = Instant::now();
    let error = execute_with(command, "sleep 10", Some(Duration::from_millis(200)))
        .expect_err("sleep should time out");
    assert!(error.contains("did not answer"), "{error}");
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
fn timed_queries_return_output_and_errors() {
    let mut command = Command::new("sh");
    command.args(["-c", "echo out; echo err >&2"]);
    let output = execute_with(command, "sh", Some(Duration::from_secs(10))).expect("runs");
    assert_eq!(output, "out\n");

    let mut command = Command::new("sh");
    command.args(["-c", "echo boom >&2; exit 3"]);
    let error = execute_with(command, "sh", Some(Duration::from_secs(10))).expect_err("fails");
    assert_eq!(error, "boom");
}

#[test]
fn bounded_read_stops_at_the_limit() {
    let mut child = Command::new("sh")
        .args(["-c", "head -c 1000000 /dev/zero"])
        .stdout(Stdio::piped())
        .spawn()
        .expect("producer");
    let mut stdout = child.stdout.take().expect("stdout");
    let (bytes, truncated) = read_bounded(&mut stdout, 1024).expect("read");
    assert_eq!(bytes.len(), 1024);
    assert!(truncated);
    drop(stdout);
    let _ = child.kill();
    let _ = child.wait();

    let (bytes, truncated) = read_bounded(&mut &b"exactly8"[..], 8).expect("read");
    assert_eq!(bytes, b"exactly8");
    assert!(!truncated);
    let (bytes, truncated) = read_bounded(&mut &b"short"[..], 8).expect("read");
    assert_eq!(bytes, b"short");
    assert!(!truncated);
}
