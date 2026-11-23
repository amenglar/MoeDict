# 示例词典

本目录只纳入一份用于开发与测试的开源示例词典；其他词典数据一律不提交（见仓库根目录的 `.gitignore`）。

## folkets_sv_en.mdx

- 内容：瑞典语 → 英语，含词条释义与发音链接（无内嵌音频）。
- 来源：由 [Folkets lexikon](http://folkets-lexikon.csc.kth.se/folkets/om.en.html)（“人民词典”，KTH 维护）的公开 XDXF 数据构建，本项目仅做格式转换。
- 许可：词典内容采用 [CC BY-SA 2.5](https://creativecommons.org/licenses/by-sa/2.5/)。
- 相关项目：[SunnyBingoMe/Sunny-Folkets-Lexikon](https://github.com/SunnyBingoMe/Sunny-Folkets-Lexikon)（同一数据集的 MDict 转换，作者 BinSun）。

## 音频

配套的 `folkets_sv_en.mdd` 约 858 MB，包含真人发音，体积过大，不随仓库提交。
缺少该文件时词典中的发音链接不可播放；如需发音，请自行构建或下载后放入本目录（会被 Git 忽略）。

## 其他词典

把任意 `.mdx`/`.mdd` 放入本目录即可用于本地测试，它们不会被提交。
