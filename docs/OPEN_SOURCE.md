# GitHub 开源发布清单

这份清单用于功能稳定后把当前工作副本整理成 GitHub 开源仓库。

## 发布前整理

1. 确认没有敏感信息：
   - 不提交 `%APPDATA%\stacker` 下的用户数据。
   - 不提交私有源地址、token、密码、公司内网配置。
   - 不提交 `node_modules`、`dist`、`src-tauri/target`。

2. 确认项目元数据：
   - `package.json` 的 `name`、`version`。
   - `src-tauri/Cargo.toml` 的 `description`、`authors`、`license`、`repository`。
   - `src-tauri/tauri.conf.json` 的 `identifier`、`productName`、`version`。
   - `src-tauri/src/update.rs` 的 `APP_REPO`，发布 GitHub Releases 后填 `owner/repo`。

3. 选择许可证：
   - 常见选择：MIT、Apache-2.0、GPL-3.0。
   - 如果希望商业/个人都能自由使用，优先 MIT 或 Apache-2.0。
   - 确定后在根目录新增 `LICENSE`。

4. 跑基线检查：
   ```powershell
   npm run lint
   npm run build
   cd src-tauri
   cargo fmt --check
   cargo check
   ```

## 初始化仓库

在项目根目录执行：

```powershell
git init
git add .
git status
git commit -m "Initial open source release"
```

创建 GitHub 空仓库后：

```powershell
git branch -M main
git remote add origin https://github.com/<owner>/<repo>.git
git push -u origin main
```

## 打第一个版本

建议先发布 `v0.1.0`：

```powershell
git tag v0.1.0
git push origin v0.1.0
```

然后在 GitHub Releases 中创建 release，上传 Tauri 构建产物。

## 发布后补齐

- 把 `APP_REPO` 改成真实 `owner/repo`，让应用内“检查更新”可用。
- 在 README 增加截图、下载链接和已知限制。
- 如要自动构建安装包，再加入 GitHub Actions。
