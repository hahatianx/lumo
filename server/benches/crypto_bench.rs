use bytes::Bytes;
use criterion::{BatchSize, Criterion, black_box, criterion_group, criterion_main};
use server::global_var::{ENV_VAR, LOGGER_CELL};
use server::utilities::temp_dir::TmpDirGuard;
use std::path::PathBuf;

// We benchmark the in-memory AES encrypt/decrypt functions.
// They depend on ENV_VAR for key derivation; initialize it once.
fn ensure_env() -> TmpDirGuard {
    use server::config::{Config, EnvVar};
    use server::global_var::ENV_VAR;

    let tmp_dir = PathBuf::from("D://SharedDisc/test").join("age-bench");
    let _ = std::fs::create_dir_all(&tmp_dir);
    let guard = TmpDirGuard::from(tmp_dir.clone());
    if ENV_VAR.get().is_none() {
        let mut cfg = Config::new();
        cfg.identity.machine_name = "bench-machine".into();
        cfg.identity.private_key_loc = "~/.keys/priv".into();
        cfg.identity.public_key_loc = "~/.keys/pub".into();
        cfg.connection.conn_token = "BENCH_TOKEN".into();
        cfg.app_config.working_dir = tmp_dir.to_str().unwrap().into();
        let ev = EnvVar::from_config(&cfg).expect("EnvVar::from_config should succeed");
        let _ = ENV_VAR.set(ev);
    }

    guard
}

fn bench_encrypt_decrypt(c: &mut Criterion) {
    let _guard = ensure_env();
    use server::utilities::crypto::{decrypt, encrypt};

    let sizes = [1024usize, 1024 * 1024]; // 1 KiB, 1 MiB
    let iv = [0xABu8; 16];

    for &sz in &sizes {
        let label_enc = format!(
            "encrypt_{}_{}B",
            ENV_VAR.get().unwrap().get_working_dir(),
            sz
        );
        let label_dec = format!(
            "decrypt_{}_{}B",
            ENV_VAR.get().unwrap().get_working_dir(),
            sz
        );
        let data = vec![0x55u8; sz];
        let data_bytes = Bytes::from(data);

        c.bench_function(&label_enc, |b| {
            b.iter(|| {
                let ct = encrypt(black_box(data_bytes.clone()), &iv).expect("encrypt ok");
                black_box(ct)
            })
        });

        // Pre-compute ciphertext for decrypt benchmark
        let ciphertext = encrypt(data_bytes.clone(), &iv).expect("encrypt ok");
        c.bench_function(&label_dec, |b| {
            b.iter(|| {
                let pt = decrypt(black_box(ciphertext.clone()), &iv).expect("decrypt ok");
                black_box(pt)
            })
        });
    }
}

fn format_sz(sz: usize) -> String {
    if sz < 1024 {
        format!("{}B", sz)
    } else if sz < 1024 * 1024 {
        format!("{:.1}KiB", sz as f64 / 1024.0)
    } else if sz < 1024 * 1024 * 1024 {
        format!("{:.1}MiB", sz as f64 / 1024.0 / 1024.0)
    } else {
        format!("{:.1}GiB", sz as f64 / 1024.0 / 1024.0 / 1024.0)
    }
}

fn bench_encrypt_decrypt_file(c: &mut Criterion) {
    let _guard = ensure_env();
    use server::utilities::crypto::{f_from_encryption, f_to_encryption};

    let sizes = [
        1024usize,
        1024 * 1024,
        100 * 1024 * 1024,
        // 1024 * 1024 * 1024, // Too much damage to my SSD :(
    ];
    const BAR: usize = 1024 * 1024 + 1;
    for &sz in &sizes {
        let label_enc = format!(
            "encrypt_file_{}_{}",
            ENV_VAR.get().unwrap().get_working_dir(),
            format_sz(sz)
        );

        let tmp_f = PathBuf::from(ENV_VAR.get().unwrap().get_working_dir())
            .join(format!("bench-file-{}B", sz));
        let tmp_t = PathBuf::from(ENV_VAR.get().unwrap().get_working_dir())
            .join(format!("bench-to-file-{}B", sz));
        let data = vec![0x55u8; sz];
        std::fs::write(&tmp_f, &data).expect("write file ok");

        c.bench_function(&label_enc, |b| {
            b.iter_batched(
                || {
                    let _ = std::fs::remove_file(&tmp_t);
                },
                |_data| {
                    let r = tokio_test::block_on(f_to_encryption(&tmp_f, &tmp_t, "benchmark"));
                    black_box(r).expect("Encryption failed");
                },
                BatchSize::PerIteration,
            )
        });
    }
}

criterion_group!(benches, bench_encrypt_decrypt, bench_encrypt_decrypt_file);
criterion_main!(benches);
