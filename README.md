# Helm

一个使用 Rust 和 egui/eframe 编写的跨平台本地密码保险箱.

## 安全模型

- 保险箱保存一个公开随机 salt, 通过 Argon2id 从统一密码派生 256-bit 内存密钥; 每次输入变化只派生一次.
- 每条记录使用独立随机 nonce 的 ChaCha20 流加密密码.
- 磁盘仅保存名称、KDF salt、nonce 和密文的 Base64 表示, 不保存统一密码、hash、派生密钥或校验值.
- 按需求不使用认证标签: 错误统一密码仍会产生等长的错误解密字节. 无效 UTF-8 会以 Base64 标记显示.
- 统一密码和新增密码在应用退出时清零; 操作系统、GUI 框架或分配器仍可能产生无法由应用保证清除的临时内存副本.

该设计无法判断统一密码是否正确, 也不能检测密文是否被篡改. 这是满足“错误密码仍返回解密结果”的直接代价.

## 运行

```bash
cargo run --release
```

数据位置由平台数据目录决定. Linux 默认为 `~/.local/share/io.github.eulcau.helm/vault.json`, Windows 位于用户的 Roaming AppData 下.

## 测试

```bash
cargo fmt --check
cargo test
```

## 生成安装包

```bash
./scripts/package.sh arch
./scripts/package.sh debian
./scripts/package.sh fedora
./scripts/package.sh windows
```

Arch 构建需要 `makepkg`; Debian 和 Fedora 构建器缺失时脚本会安装对应 Cargo 工具. MSI 使用新版 WiX Toolset 的 `wix.exe build` 生成, 必须在装有 .NET SDK 和 WiX v4 或更高版本的 Windows Git Bash 环境运行:

```bash
dotnet tool install --global wix
wix --version
./scripts/package.sh windows
```

项目不再依赖 `cargo-wix`、`candle.exe` 或 `light.exe`. `all` 只适合具备全部原生打包环境的 CI 矩阵; 不支持从单一 Linux 主机直接生成所有原生安装包.
