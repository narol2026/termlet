# GitHub 开源发布检查记录

最后复核：2026-11-14
复核基线：`main@f151b02`；正式 Release 为 `v1.0.0`。
用途：这是发布前检查表，不是部署状态或发布日期承诺。每次准备公开仓库或
创建 tag 前，应按下面的命令重新检查，并更新本文件。

当前仓库状态：

- GitHub 仓库已公开，`v1.0.0` tag 与 GitHub Release 已发布。
- 仓库 description、homepage、topics、Issues 以及 `bug`/`enhancement` labels
  已配置。
- 工作流使用完整 commit SHA 固定 GitHub Actions，Dependabot 已配置 Cargo、
  npm 和 Actions 的每周检查。
- RustSec、npm audit 和 Dependabot 当前均无已知漏洞；RustSec 仍有上游
  unmaintained/unsound 预警，详见后文。

## 当前已验证事项

- [x] 默认分支为 `main`。
- [x] `cargo fmt --all -- --check` 通过。
- [x] `cargo clippy --all-targets --all-features --locked -- -D warnings`
  通过。
- [x] `cargo test --locked` 通过：740 passed，0 failed，4 ignored。
- [x] 应用图标检查通过，ICO、ICNS 和 Linux PNG 的尺寸/内容一致。
- [x] `python3 tools/licenses/gen_deb_copyright.py --check` 通过。
- [x] 642 个翻译键在 `en`、`zh-CN`、`zh-TW` 中齐全，占位符一致。
- [x] 仓库跟踪文件体积正常，没有误提交 `target/` 或 `node_modules/`。

## P0：公开源码前必须完成

### GitHub 仓库状态与治理

- [x] 将 `narol2026/termlet` 从私有切换为公开。

- [x] 重新登录本机 GitHub CLI，并移除失效的旧账号 session。
- [x] 在仓库公开后立即开启并验证 GitHub 私有漏洞报告。

  [SECURITY.md](SECURITY.md) 和 Issue 模板都指向
  `Security -> Advisories -> Report a vulnerability`。私有漏洞报告通常要求仓库
  已公开，因此切换可见性后应立刻配置并从无权限账号验证入口。

- [x] 开启 secret scanning、push protection 和 Dependabot alerts/security
  updates。
- [ ] 为 `main` 配置分支保护或 ruleset，要求 CI 和 CODEOWNERS 审核。

  当前私有仓库返回 GitHub API 403：免费账号需要仓库转为 public 或使用 Pro
  才能启用 branch protection。若暂不升级，应在公开后立即配置。

- [x] 确认 Issue 模板使用的 `bug`、`enhancement` labels 已创建。
- [x] 设置仓库 description、topics 和 homepage。

  description 为 “A cross-platform desktop terminal focused on SSH session
  management”；homepage 指向仓库地址；topics 已设置。

- [ ] 在 GitHub 的 **Settings -> Social preview** 中上传
  `assets/screenshots/termlet-social-preview.jpg`。

### 第三方许可证合规

- [x] 修正 fork 数量。

  [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md)、
  [CONTRIBUTING.md](CONTRIBUTING.md) 和 [PATCHES.md](PATCHES.md) 现在都按
  `Cargo.toml` 的实际状态列出五个 fork：`iced` family、`iced_drop`、
  `iced_term`、`vte`、`alacritty_terminal`。

- [x] 在 [NOTICE](NOTICE) 中补充实际使用的 `iced` family 归属信息。
- [ ] 处理“声明许可证但未随 crate 提供许可证正文”的依赖。

  当前生成器报告 825 个依赖中有 95 个没有随 crate 提供许可证正文；
  `tools/licenses/upstream/` 目录存在，但其中没有 Git 跟踪的补充文件。

- [ ] 给 `tools/licenses/gen_third_party_licenses.py` 增加真正的 `--check`
  模式。当前脚本会忽略 `--check` 并直接重写清单；CI 也只检查 Debian
  copyright，没有检查第三方许可证清单是否新鲜或是否仍有缺口。
- [ ] 人工复核 `tools/licenses/upstream/` 的来源记录。

  补文和构造文本建立后，需要维护者或法律顾问抽查其来源、Lucide 的 MIT+ISC
  组合，以及 `libfuzzer-sys` 的 NCSA 文本，不能仅以自动检查代替法律判断。

- [ ] 在 CI 中增加 `cargo-deny` 或等价检查，覆盖 licenses、advisories 和
  dependency sources。

### 文档同步

- [x] 删除 [README.md](README.md) 中“重连后提示 `cd` 回旧目录”的描述。

  README 和应用内 Shell Integration 提示均已删除这项已移除的功能。

- [x] 修正 `packaging/windows/README.md` 中已过时的 OpenSSH 前置要求。
  当前文档明确说明 SSH 终端由内置 `termlet-bridge.exe` 启动，不依赖系统
  OpenSSH Client。
- [x] 明确产品支持边界。README 现在将 Windows 标为源码构建、未覆盖 CI、
  暂无官方发布包，并将 Linux 和 macOS 列为当前支持的构建目标。
- [x] 补充已知的 Linux 二进制基线：CI 在 Ubuntu 22.04/glibc 2.35 上构建。
- [ ] 在正式发布 macOS 二进制前，明确并验证最低运行时版本；当前 README
  只记录了 CI 使用 macOS 15，尚未承诺更低的运行时下限。

## P1：发布 1.0 二进制前完成

- [x] 在 `CHANGELOG.md` 中增加正式的 `1.0.0` 版本段，并清理 `Unreleased`。
- [x] 创建并推送与 `Cargo.toml` 版本一致的 `v1.0.0` tag。

- [x] 构建并验证 Linux x86_64、Linux aarch64、macOS Intel 和 macOS Apple
  Silicon 产物，然后发布到 `v1.0.0`。

  首次 release workflow 的 checksum 步骤把嵌套的 `debian/` 目录当成文件；
  安装包均已成功构建，Release 使用这些 CI 产物发布，main 上的工作流已修复。
- [ ] 配置 macOS 代码签名和 notarization。

  当前 release workflow 明确说明 DMG 未签名、未公证，普通用户首次启动会遇到
  Gatekeeper 阻止。

- [x] 如果首版提供 Windows 安装包，将其加入 release workflow；否则在下载说明中
  明确 Windows 暂不提供正式二进制。
- [ ] 在 CI 中启动真实 `sshd`，运行当前 4 个被忽略的 SSH/SFTP 集成测试。
- [x] 为 Linux 与 macOS 发布产物增加可验证的签名，或者明确仅提供
  `SHA256SUMS`。
- [x] 增加“下载与安装”章节，直接链接 Release 中的 `.deb` 和 `.dmg`。

## Windows 转正式支持前必须完成

- [x] 为 `%USERPROFILE%\.termlet` 实现显式 Windows DACL。

  [src/core/paths.rs](src/core/paths.rs) 现在为目录和敏感文件设置仅当前用户
  的受保护 DACL；目录 ACE 会继承到后续创建的文件，SQLite sidecar 也会在
  启用 WAL 后再次收紧。

- [x] 增加 Windows CI：build、test、clippy、打包和安装包冒烟测试。

  `windows-latest` 原生 job 会运行 Clippy、完整测试、静态 CRT 构建、portable
  ZIP 和 Inno Setup 安装包构建，并执行安装、App Paths、卸载和用户数据保留
  冒烟测试。
- [x] 将 [packaging/windows/termlet.iss](packaging/windows/termlet.iss) 的
  `MinVersion=10.0` 提升到支持 ConPTY 的 Windows 10 1809
  (`10.0.17763`) 或更高。
- [x] 替换 [src/core/open.rs](src/core/open.rs) 中的 `cmd.exe /c start` URL
  打开方式，避免 `&version=...&platform=...` 被命令解释器拆分。
- [ ] 在干净的 Windows 10 22H2 和 Windows 11 VM 中验证 portable ZIP 与
  installer。
- [ ] 完成 Windows 代码签名，或者把 SmartScreen 警告明确写入已知限制。
- [ ] 验证 IME、粘贴、DPI、多显示器、ConPTY resize、安装/卸载和本地文件传输。
- [ ] 覆盖 Windows 非法本地文件名、保留设备名、符号链接和 tar 解包边界。
- [x] 确认哪些 Windows shell 支持 OSC 7/633；不支持时隐藏或禁用相关设置。

## 供应链与 CI 加固

- [ ] 增加 `.github/dependabot.yml`，至少覆盖 Cargo、npm 和 GitHub Actions。
- [ ] 处理 RustSec 扫描发现的 6 个漏洞。

  2026-11-02 使用 `cargo-audit 0.22.2` 扫描 `Cargo.lock`，加载 1395 条公告，
  扫描 831 个依赖后以非零状态退出。

  | Crate | Advisory | 严重性 | 依赖路径 |
  | --- | --- | --- | --- |
  | `crossbeam-epoch 0.9.18` | RUSTSEC-2026-0204 | 未提供 CVSS | `image` / `rust-i18n-support` |
  | `crypto-bigint 0.7.0-rc.28` | RUSTSEC-2026-0387 | 未提供 CVSS | `rsa` -> `russh` |
  | `ctutils 0.4.2` | RUSTSEC-2026-0385 | 未提供 CVSS | RustCrypto -> `russh` |
  | `quick-xml 0.39.4` | RUSTSEC-2026-0195 | 7.5 high | `wayland-scanner` 编译期工具 |
  | `quick-xml 0.39.4` | RUSTSEC-2026-0194 | 7.5 high | `wayland-scanner` 编译期工具 |
  | `rsa 0.10.0-rc.16` | RUSTSEC-2023-0071 | 5.9 medium | RSA 密钥 -> `russh` |

  `crypto-bigint`、`ctutils` 和 `rsa` 位于 SSH 加密/认证的运行时依赖路径，
  应作为 P0 安全事项处理。`quick-xml` 当前通过 Wayland 协议生成器在构建期
  使用，风险与直接解析不可信 XML 不同，但升级仍应完成。

- [ ] 评估 21 条 RustSec warning。

  其中包含 `anyhow`、`event-listener`、`lru`、`memmap2`、`tokio` 的
  unsound 警告；`basic-toml`、`paste`、`proc-macro-error2`、`rustybuzz`、
  `ttf-parser`、`zbus_names`、`zvariant` 的 unmaintained 警告；以及
  `aes 0.9.0`、`chacha20 0.10.0`、`clipboard-win 5.4.1`、`der 0.8.0`、
  `spin 0.9.8` 的 yanked 警告。

- [ ] 将 `cargo-audit` 或 `cargo-deny` 接入 CI，并记录明确接受的例外。

  当前只能手动复现：

  ```bash
  cargo install cargo-audit --locked
  cargo audit
  ```

- [x] 将 GitHub Actions 从可移动的 `@vN` tag 固定到完整 commit SHA，
  尤其是拥有 `contents: write` 的 release job。
- [ ] 考虑把 release job 放入受保护 environment，限制 tag 发布权限。
- [ ] 定期验证 fork 的许可证变化、upstream rebase 和固定 commit 的可访问性。

## 建议执行顺序

1. 复核已补齐的许可证正文来源和公开文档。
2. 配置 GitHub 安全功能、分支保护和 secret scanning。
3. 将仓库切换为 public，先开放源码。
4. 增加依赖安全、许可证和真实 SSH 集成测试。
5. 决定 macOS 签名和 Windows 发布范围后，发布 `v1.0.0` 二进制。

公开源码不应被 Windows 专项工作阻塞，但在 README 明确 Windows 未正式支持之前，
不应同时宣传 Windows 1.0 发布包。

## GitHub 设置执行方式

以下操作会改变仓库或账号状态，因此本次审计没有自动执行。维护者确认后可以按
下面的顺序完成：

1. 重新认证：

   ```bash
   gh auth login -h github.com
   gh auth status
   ```

2. 在仓库仍为私有时先配置 Dependabot security updates。免费私有仓库无法启用
   branch protection；如有 GitHub Pro，可以提前配置 ruleset。

3. 在 GitHub 的 **Settings -> General -> Danger Zone** 中确认测试分支、README
   和许可证无误后，将仓库改为 Public。

4. 仓库公开后立即在 **Settings -> Code security and analysis** 中开启并验证：

   - Private vulnerability reporting
   - Secret scanning
   - Push protection
   - Dependabot alerts
   - Dependabot security updates

   私有漏洞报告也可以使用：

   ```bash
   gh api --method PUT \
     repos/narol2026/termlet/private-vulnerability-reporting
   ```

5. 立即为 `main` 的 ruleset 要求通过 CI、至少一名 CODEOWNER 审核、禁止强制
   推送和删除分支。需要保护的 job 名称来自 `.github/workflows/ci.yml`，包括
   `fmt + clippy`、`test (linux x86_64)`、`msrv (cargo check)` 和 build matrix。

6. 确认 Issue 模板使用的 labels：

   ```bash
   gh label create bug --color d73a4a --description "Something is broken"
   gh label create enhancement --color a2eeef --description "New feature or request"
   ```

7. 检查 README 预览，并在 **Settings -> Social preview** 上传
   `assets/screenshots/termlet-social-preview.jpg`，然后从未认证
   浏览器确认 README、徽章、安全入口和 Release 链接均可访问。
