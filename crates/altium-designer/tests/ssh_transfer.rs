//! Explicit integration check: uses the configured Windows SSH host, without running Altium.
use altium_designer::config::Config;
use altium_designer::host::{Host, ROOT};
use altium_designer::ssh::{SshConfig, SshHost};

#[test]
#[ignore = "requires an explicitly configured Windows SSH host and pinned host key"]
fn ssh_file_transfers_keep_temporary_workspaces_alive() {
    let cfg = Config::load().unwrap();
    let host = SshHost::connect(SshConfig::from(&cfg).expect("configure designer ssh_host first")).unwrap();
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let path = format!(r"{ROOT}\transfer-tests\{}-{stamp}.txt", std::process::id());
    let other = format!("{path}.other");
    let payload = b"ace synthetic transfer regression payload\r\n";
    let result = (|| -> altium_core::Result<()> {
        host.write(&path, payload)?;
        assert_eq!(host.read(&path)?.as_deref(), Some(payload.as_slice()));
        host.write_many(&[(path.clone(), b"updated".to_vec()), (other.clone(), b"second".to_vec())])?;
        assert_eq!(host.read_many(&[path.clone(), other.clone()])?, vec![Some(b"updated".to_vec()), Some(b"second".to_vec())]);
        Ok(())
    })();
    for file in [&path, &other] { host.remove(file).unwrap(); }
    result.unwrap();
}
