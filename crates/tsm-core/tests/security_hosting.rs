mod common;
use common::*;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use tsm_core::hosting::providers::MockProvider;
use tsm_core::hosting::HostingProvider;
use tsm_core::model::*;

#[tokio::test]
async fn secrets_are_encrypted_and_redacted() {
    let root = tempfile::tempdir().unwrap();
    let e = engine(root.path()).await;
    let token = "eyJhIjoiMTIzNDU2Nzg5MCIsInQiOiJhYmNkZWYifQ-SuperSecretTunnelToken123";
    e.secrets.set("hosting.test.cloudflare", token).unwrap();
    let raw = e.secrets.raw_file_contents();
    assert!(!raw.contains(token), "plaintext must not be stored");
    assert!(!raw.contains("SuperSecret"));
    assert_eq!(e.secrets.get("hosting.test.cloudflare").unwrap().as_deref(), Some(token));
    // known secret + heuristic patterns are redacted from logs
    e.logs.system("p", format!("connecting with {token}"));
    e.logs.system("p", "ngrok --authtoken 2abcDEFghiJKLmno_pqrsTUV");
    e.logs.system("p", "db password=hunter2hunter2 ok");
    e.logs.system("p", "mysql://root:topsecret@127.0.0.1/db");
    let lines = e.logs.lines("p", None, 10);
    for l in &lines {
        assert!(!l.text.contains("SuperSecret"), "{}", l.text);
        assert!(!l.text.contains("2abcDEF"), "{}", l.text);
        assert!(!l.text.contains("hunter2"), "{}", l.text);
        assert!(!l.text.contains("topsecret"), "{}", l.text);
    }
    // a new engine (app restart) still redacts the stored secret
    drop(e);
    let e2 = engine(root.path()).await;
    e2.logs.system("p", format!("leak? {token}"));
    assert!(e2.logs.lines("p", None, 1)[0].text.contains("[REDACTED]"));
}

#[tokio::test(flavor = "multi_thread")]
async fn process_output_with_tokens_is_redacted() {
    let root = tempfile::tempdir().unwrap();
    let folder = tempfile::tempdir().unwrap();
    let e = engine(root.path()).await;
    let port = free_port();
    let p = e.projects.create(test_project("Leaky", folder.path(), port, &["--print", "api_key=ABCDEF1234567890"])).unwrap();
    e.start(&p.id).await.unwrap();
    wait_status(&e, &p.id, ServerStatus::Running, 20).await;
    tokio::time::sleep(Duration::from_millis(600)).await;
    let lines = e.logs.lines(&p.id, None, 100);
    assert!(lines.iter().any(|l| l.text.contains("api_key=[REDACTED]")), "{lines:#?}");
    assert!(lines.iter().all(|l| !l.text.contains("ABCDEF1234567890")));
    let export = root.path().join("log.txt");
    e.logs.export(&p.id, &export).unwrap();
    assert!(!std::fs::read_to_string(export).unwrap().contains("ABCDEF1234567890"));
    e.stop(&p.id, true).await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn provider_is_not_online_just_because_process_runs() {
    let root = tempfile::tempdir().unwrap();
    let folder = tempfile::tempdir().unwrap();
    let mock = Arc::new(MockProvider::default());
    let e = engine_with(root.path(), vec![mock.clone() as Arc<dyn HostingProvider>]).await;
    let port = free_port();
    let mut p = test_project("Pub", folder.path(), port, &[]);
    p.hosting.provider = ProviderId::Mock;
    let p = e.projects.create(p).unwrap();

    // hosting requires a running server
    assert!(e.hosting_start(&p.id).await.is_err());
    e.start(&p.id).await.unwrap();
    wait_status(&e, &p.id, ServerStatus::Running, 20).await;

    // public provider without exposure consent is refused
    let err = e.hosting_start(&p.id).await.unwrap_err();
    assert!(err.to_string().contains("confirm"), "{err}");
    let mut p2 = e.projects.get(&p.id).unwrap();
    p2.hosting.exposure_consent = true;
    e.projects.update(p2).unwrap();

    // agent "running" but endpoint unverified -> Connecting, never PublicOnline
    mock.endpoint_ok.store(false, Ordering::SeqCst);
    let st = e.hosting_start(&p.id).await.unwrap();
    assert_eq!(st.status, HostingStatus::Connecting);
    tokio::time::sleep(Duration::from_secs(4)).await;
    assert_ne!(e.hosting.state(&p.id).status, HostingStatus::PublicOnline);

    // endpoint verified -> PublicOnline (monitor picks it up)
    mock.endpoint_ok.store(true, Ordering::SeqCst);
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while e.hosting.state(&p.id).status != HostingStatus::PublicOnline {
        assert!(std::time::Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    assert_eq!(e.overview().public_online, 1);

    // stopping the server stops hosting too
    e.stop(&p.id, false).await.unwrap();
    assert_eq!(e.hosting.state(&p.id).status, HostingStatus::Disabled);
    assert!(!mock.agent_running.load(Ordering::SeqCst));
}

#[test]
fn pasted_tokens_and_hostnames_are_cleaned() {
    use tsm_core::engine::{clean_hostname, clean_token};
    let tok = "eyJhIjoiYWJjZGVmIiwidCI6IjEyMzQ1Njc4OTAiLCJzIjoiWFlaIn0=";
    assert_eq!(clean_token(ProviderId::Cloudflare, &format!("cloudflared.exe service install {tok}")), tok);
    assert_eq!(clean_token(ProviderId::Cloudflare, &format!("sudo cloudflared tunnel run --token {tok}\n")), tok);
    assert_eq!(clean_token(ProviderId::Ngrok, "ngrok config add-authtoken 2abcDEF_xyz"), "2abcDEF_xyz");
    assert_eq!(clean_hostname("https://Shop.Example.com/login.php"), "shop.example.com");
    assert_eq!(clean_hostname("shop.example.com"), "shop.example.com");
}

#[tokio::test(flavor = "multi_thread")]
async fn quick_publish_requires_consent_and_starts_server() {
    let root = tempfile::tempdir().unwrap();
    let folder = tempfile::tempdir().unwrap();
    let e = engine(root.path()).await;
    let p = e.projects.create(test_project("QP", folder.path(), free_port(), &[])).unwrap();
    // public modes need consent
    let err = e.quick_publish(&p.id, "quick", None, None, false, false).await.unwrap_err();
    assert_eq!(err.code(), "approval_required");
    // domain mode without token is refused before anything starts
    let err = e.quick_publish(&p.id, "domain", None, Some("x.example.com".into()), true, false).await.unwrap_err();
    assert!(err.to_string().contains("token"), "{err}");
    assert_eq!(e.supervisor.state(&p.id).status, ServerStatus::Stopped);
}

#[tokio::test]
async fn provider_protocol_rules() {
    use tsm_core::hosting::{capabilities, common_validation, HostingTarget, Support};
    let mk = |proto: Protocol, provider: ProviderId, port: u16| HostingTarget {
        project: Project { protocol: proto, port, hosting: HostingConfig { provider, exposure_consent: true, ..Default::default() }, ..Default::default() },
        token: None,
        lan_ip: None,
        public_ip: None,
        nat: None,
        server_running: true,
        firewall_rule: None,
    };
    // ngrok: no UDP; cloudflare tunnel: no public UDP, TCP only conditional
    assert_eq!(capabilities(ProviderId::Ngrok).for_protocol(Protocol::Udp).support, Support::No);
    assert_eq!(capabilities(ProviderId::Ngrok).for_protocol(Protocol::Tcp).support, Support::Conditional);
    assert_eq!(capabilities(ProviderId::Cloudflare).for_protocol(Protocol::Udp).support, Support::No);
    assert_eq!(capabilities(ProviderId::Cloudflare).for_protocol(Protocol::Tcp).support, Support::Conditional);
    assert_eq!(capabilities(ProviderId::Playit).for_protocol(Protocol::Udp).support, Support::Yes);
    assert!(!common_validation(&mk(Protocol::Udp, ProviderId::Ngrok, 19132), &capabilities(ProviderId::Ngrok)).is_empty());
    // database ports never published
    let errs = common_validation(&mk(Protocol::Tcp, ProviderId::Frp, 3306), &capabilities(ProviderId::Frp));
    assert!(errs.iter().any(|e| e.contains("never published")));
    // localhost needs no consent
    let mut t = mk(Protocol::Http, ProviderId::Local, 8001);
    t.project.hosting.exposure_consent = false;
    assert!(common_validation(&t, &capabilities(ProviderId::Local)).is_empty());
}

#[tokio::test]
async fn games_are_never_sent_to_cloudflare_and_playit_needs_a_key() {
    let root = tempfile::tempdir().unwrap();
    let folder = tempfile::tempdir().unwrap();
    let e = engine(root.path()).await;
    let mut game = test_project("Game", folder.path(), free_port(), &[]);
    game.protocol = Protocol::Tcp;
    let game = e.projects.create(game).unwrap();

    for mode in ["quick", "domain"] {
        let err = e.quick_publish(&game.id, mode, None, Some("mc.example.com".into()), true, false).await.unwrap_err();
        assert!(err.to_string().contains("playit"), "{mode}: {err}");
    }
    let err = e.quick_publish(&game.id, "playit", None, None, true, false).await.unwrap_err();
    assert!(err.to_string().to_lowercase().contains("secret"), "{err}");
    // a bad address is refused before anything starts
    let err = e.quick_publish(&game.id, "playit", Some("agent-secret-123456".into()), Some("not an address/x".into()), true, false).await.unwrap_err();
    assert!(err.to_string().contains("joinmc.link"), "{err}");
    // consent is still required
    assert!(e.quick_publish(&game.id, "playit", None, None, false, false).await.is_err());

    // ngrok: TCP games need the authtoken first
    let err = e.quick_publish(&game.id, "ngrok", None, None, true, false).await.unwrap_err();
    assert!(err.to_string().to_lowercase().contains("authtoken"), "{err}");
    // and UDP games (e.g. Bedrock) are refused: ngrok has no UDP
    let mut bedrock = test_project("Bedrock", folder.path(), free_port(), &[]);
    bedrock.protocol = Protocol::Udp;
    let bedrock = e.projects.create(bedrock).unwrap();
    let err = e.quick_publish(&bedrock.id, "ngrok", Some("fake-ngrok-token-123".into()), None, true, false).await.unwrap_err();
    assert!(err.to_string().contains("UDP"), "{err}");
    assert!(!e.supervisor.is_active(&game.id) && !e.supervisor.is_active(&bedrock.id), "nothing was started");
}