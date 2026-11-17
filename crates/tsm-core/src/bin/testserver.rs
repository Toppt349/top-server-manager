//! Tiny server used by the automated tests (std only).
//!
//! tsm-testserver --port N [--host 0.0.0.0] [--crash-after-ms N] [--exit-code N] [--print TEXT] [--udp] [--spawn-child]
//! * answers every HTTP request with `200 ok <port>`
//! * reads stdin lines: `stop` exits 0, anything else is echoed
use std::io::{BufRead, Read, Write};
use std::net::{TcpListener, UdpSocket};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let get = |k: &str| args.iter().position(|a| a == k).and_then(|i| args.get(i + 1)).cloned();
    let port: u16 = get("--port").and_then(|p| p.parse().ok()).or_else(|| std::env::var("PORT").ok().and_then(|p| p.parse().ok())).unwrap_or(18080);
    let crash_after: Option<u64> = get("--crash-after-ms").and_then(|p| p.parse().ok());
    let exit_code: i32 = get("--exit-code").and_then(|p| p.parse().ok()).unwrap_or(3);
    if let Some(t) = get("--print") {
        println!("{t}");
    }
    if let Ok(v) = std::env::var("DB_DATABASE") {
        println!("env DB_DATABASE={v}");
    }
    // --host 0.0.0.0 imitates a game server that ignores the app's listen setting
    let host = get("--host").unwrap_or_else(|| "127.0.0.1".into());
    println!("testserver listening on {host}:{port}");
    let _ = std::io::stdout().flush();

    if args.iter().any(|a| a == "--udp") {
        let sock = UdpSocket::bind(("127.0.0.1", port)).expect("bind udp");
        std::thread::spawn(move || {
            let mut buf = [0u8; 1500];
            while let Ok((n, from)) = sock.recv_from(&mut buf) {
                let _ = sock.send_to(&buf[..n], from);
            }
        });
    } else {
        let listener = TcpListener::bind((host.as_str(), port)).expect("bind tcp");
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                std::thread::spawn(move || {
                    let mut s = stream;
                    let mut buf = [0u8; 2048];
                    let _ = s.set_read_timeout(Some(std::time::Duration::from_millis(500)));
                    let _ = s.read(&mut buf);
                    let body = format!("ok {port}");
                    let _ = write!(s, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
                });
            }
        });
    }
    if args.iter().any(|a| a == "--spawn-child") {
        // grandchild process to prove tree-kill via Job Objects
        let exe = std::env::current_exe().unwrap();
        let _child = std::process::Command::new(exe).args(["--port", &(port + 1000).to_string()]).spawn();
    }
    if let Some(ms) = crash_after {
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(ms));
            eprintln!("ERROR simulated crash");
            std::process::exit(exit_code);
        });
    }
    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim() == "stop" {
            println!("stopping gracefully");
            let _ = std::io::stdout().flush();
            std::process::exit(0);
        }
        println!("echo: {line}");
        let _ = std::io::stdout().flush();
    }
    // stdin closed (re-attached / no pipe): keep serving
    loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
    }
}
