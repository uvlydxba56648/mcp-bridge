pub mod gateway;
pub mod logbus;
pub mod mcp;
pub mod netutil;
pub mod oauth;
pub mod tunnel;

/// 生成访问密钥:sk-mcp- + 32 位十六进制(128 bit 随机)
pub fn gen_api_key() -> String {
    use rand::RngCore;
    let mut b = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut b);
    let hex: String = b.iter().map(|x| format!("{:02x}", x)).collect();
    format!("sk-mcp-{}", hex)
}
