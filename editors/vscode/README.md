# biomiya for VS Code

面向 **S-next 0.1** 的轻量语言插件，自动识别 `.vt` 文件。

- 高亮关键字、基础类型、声明、调用、整数和字符串转义。
- 支持嵌套块注释、中文注释和字符串。
- 括号匹配与自动补全、注释快捷键、基础缩进、按缩进折叠和 `// #region` 区域折叠。
- 输入 `module`、`fn`、`struct`、`enum`、`unsafe` 后从建议列表选择代码片段。

这是纯声明式插件，没有运行时代码、后台进程或编译器依赖，也不修改编译器。尚不提供编译诊断、格式化、跳转定义、类型悬停或 LSP。大写名字的类型颜色是词法提示，不代表语义解析。基础缩进不是格式化器，复杂多行注释等场景可能需要手动调整。

历史版本也使用 `.vt` 扩展名，但本插件只按 S-next 规则着色。不接管通用 `.build` 文件或 `.s0` 汇编。

## 安装

在 VS Code 扩展面板菜单中选择 **Install from VSIX… / 从 VSIX 安装…**，选择本目录生成的 `biomiya-language-0.1.0.vsix`。或者在本目录执行：

```sh
code --install-extension ./biomiya-language-0.1.0.vsix
```

打开 `.vt` 后，右下角语言模式应显示 **biomiya (S-next)**。若被其他文件关联覆盖，可点击语言模式手动选择，或在工作区设置 `"files.associations": { "*.vt": "biomiya" }`。颜色由当前 VS Code 主题决定。

本包仅供本地安装，`biomiya-local` 是本地扩展标识，未发布到 Marketplace。

## 开发与打包

在本目录执行（开发依赖需要 Node.js 22 或更新版本）：

```sh
npm ci
npm test
npm run package
```

测试使用 VS Code 的 TextMate / Oniguruma 分词组件，检查嵌套注释、字符串转义、输入未完成时的状态恢复和整数后缀。VSIX 不包含这些开发依赖。

在 VS Code 中以本目录为工作区按 F5，可打开带本插件和示例文件的 Extension Development Host。手动检查主题着色、注释快捷键、括号配对与回车缩进。

语法依据仓库 `docs/reference/s-next-0.1.md` 与当前 Q 词法器。扩展结构遵循 VS Code 的 [Syntax Highlight Guide](https://code.visualstudio.com/api/language-extensions/syntax-highlight-guide) 和 [Language Configuration Guide](https://code.visualstudio.com/api/language-extensions/language-configuration-guide)。
