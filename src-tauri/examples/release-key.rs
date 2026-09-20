//! 发布签名工具（只在本机发版时用，不进应用二进制）。
//!
//! 生成密钥对（会提示设置密码，直接回车表示不加密）：
//!   cargo run --example release-key -- keygen %USERPROFILE%\.stacker\release-signing.key
//! 给文件签名，签名打到标准输出：
//!   cargo run --example release-key -- sign %USERPROFILE%\.stacker\release-signing.key Stacker-setup.exe
//! 上传前自检：用程序里内置的那把公钥验一遍产物和它的 .minisig：
//!   cargo run --example release-key -- verify Stacker-setup.exe Stacker-setup.exe.minisig
//!
//! 私钥设了密码才会交互式询问；没设密码的私钥不打扰，脚本里也能直接跑。
//! 也可以用环境变量 STACKER_SIGNING_PASSWORD 直接给出密码。
//!
//! 私钥不要放进仓库，也不要放进 CI：应用里内置的是公钥，私钥留在发版这台机器上。
use std::fs::File;
use std::io::{BufReader, Write};
use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("keygen") if args.len() == 2 => keygen(Path::new(&args[1])),
        Some("sign") if args.len() == 3 => sign(Path::new(&args[1]), Path::new(&args[2])),
        Some("verify") if args.len() == 3 => verify(Path::new(&args[1]), Path::new(&args[2])),
        _ => Err(usage()),
    };
    if let Err(message) = result {
        eprintln!("{message}");
        std::process::exit(2);
    }
}

/// 有环境变量就用它，没有就返回 None 让 minisign 交互式询问。
fn password() -> Option<String> {
    std::env::var("STACKER_SIGNING_PASSWORD").ok()
}

/// 解开私钥：给了密码就用密码；没给就先按「没加密」试一次，
/// 真加密了才交互式询问，免得没设密码的私钥也要敲一次回车。
fn open_secret(text: &str) -> Result<minisign::SecretKey, String> {
    let parse =
        || minisign::SecretKeyBox::from_string(text).map_err(|e| format!("私钥格式不对：{e}"));
    if let Some(given) = password() {
        return parse()?
            .into_secret_key(Some(given))
            .map_err(|e| format!("私钥解不开：{e}"));
    }
    if let Ok(key) = parse()?.into_secret_key(Some(String::new())) {
        return Ok(key);
    }
    parse()?
        .into_secret_key(None)
        .map_err(|e| format!("私钥解不开：{e}"))
}

fn usage() -> String {
    "用法：\n  release-key keygen <私钥路径>\n  release-key sign <私钥路径> <文件>".into()
}

fn keygen(secret_path: &Path) -> Result<(), String> {
    if secret_path.exists() {
        return Err(format!(
            "{} 已存在；换个路径，或先把旧私钥移走",
            secret_path.display()
        ));
    }
    if let Some(parent) = secret_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let public_path = secret_path.with_extension("pub");
    let pk_file = File::create(&public_path).map_err(|e| e.to_string())?;
    let sk_file = File::create(secret_path).map_err(|e| e.to_string())?;
    let pair = minisign::KeyPair::generate_and_write_encrypted_keypair(
        std::io::BufWriter::new(pk_file),
        std::io::BufWriter::new(sk_file),
        Some("Stacker release signing key"),
        password(),
    )
    .map_err(|e| format!("生成密钥失败：{e}"))?;

    println!("私钥：{}", secret_path.display());
    println!("公钥：{}", public_path.display());
    println!();
    println!("把下面这一行填进 src-tauri/src/update.rs 的 RELEASE_PUBLIC_KEY：");
    println!("{}", pair.pk.to_base64());
    Ok(())
}

fn sign(secret_path: &Path, target: &Path) -> Result<(), String> {
    let secret = std::fs::read_to_string(secret_path)
        .map_err(|e| format!("读不到私钥 {}：{e}", secret_path.display()))?;
    let secret = open_secret(&secret)?;
    let file = File::open(target).map_err(|e| format!("读不到 {}：{e}", target.display()))?;
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let signature = minisign::sign(
        None,
        &secret,
        BufReader::new(file),
        Some(&name),
        Some("Stacker release"),
    )
    .map_err(|e| format!("签名失败：{e}"))?;
    let mut out = std::io::stdout();
    out.write_all(signature.to_string().as_bytes())
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// 用应用里内置的 RELEASE_PUBLIC_KEY 验一遍，确认用户那边装得上。
fn verify(target: &Path, signature_path: &Path) -> Result<(), String> {
    let bytes = std::fs::read(target).map_err(|e| format!("读不到 {}：{e}", target.display()))?;
    let signature = std::fs::read_to_string(signature_path)
        .map_err(|e| format!("读不到 {}：{e}", signature_path.display()))?;
    stacker_lib::update::verify_release_signature(&bytes, &signature)?;
    println!("OK：{} 的签名可以被内置公钥验过", target.display());
    Ok(())
}
