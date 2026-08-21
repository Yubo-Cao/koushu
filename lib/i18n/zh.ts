import type { Messages } from "./en";

/**
 * Simplified Chinese.
 *
 * Two rules run through this file. First, keep the technical nouns in English
 * where that is what a Chinese developer actually says out loud — Markdown,
 * VAD, API key, Base URL, GGUF, Hugging Face, CPU, SQLite, LLM. Translating
 * them ("应用程序接口密钥") reads as machine output. Second, keep it short:
 * Chinese sets roughly 1.6× denser per character but the controls here are
 * sized in pixels, so a faithful full-sentence translation is what overflows a
 * button. Labels are two to four characters, sentences drop the subject.
 *
 * `Messages` is `Record<MessageKey, string>`, so a key missing from here is a
 * type error, not a runtime fallback.
 */
export const zh: Messages = {
  // ---- Shared ------------------------------------------------------------
  "common.appName": "口述",
  "common.loading": "正在启动口述",
  "common.settings": "设置",
  "common.pause": "暂停",
  "common.copy": "复制",
  "common.clear": "清除",
  "common.download": "下载",
  "common.resume": "继续下载",
  "common.installed": "已安装",
  "common.yes": "是",
  "common.no": "否",
  "common.none": "无",
  "common.unknown": "未知",

  "titlebar.minimise": "最小化",
  "titlebar.maximise": "最大化",
  "titlebar.close": "关闭",

  // ---- Model status ------------------------------------------------------
  "model.status.available": "可下载",
  "model.status.downloading": "下载中",
  "model.status.installed": "已安装",
  "model.status.paused": "已暂停",
  "model.status.error": "出错",
  "model.status.ready": "就绪",

  // ---- ASR runtimes ------------------------------------------------------
  "backend.nano": "Fun-ASR-Nano（精准）",
  "backend.sensevoice": "SenseVoiceSmall（快速）",

  // ---- Download progress -------------------------------------------------
  "download.preparing": "准备下载",
  "download.downloaded": "已下载 {size}",
  "download.model": "正在从 Hugging Face 下载模型",
  "download.paused": "下载已暂停",
  "download.pausing": "正在暂停下载…",
  "download.installed": "模型已安装",

  // ---- Main window -------------------------------------------------------
  "home.newSession": "新建会话",
  "home.noSession": "暂无会话",
  "home.sessionTitle": "会话 {time}",
  "home.sessionUntitled": "会话",
  "home.showVoiceBar": "显示悬浮语音条",

  "home.sidebar.noMatches": "没有会话符合当前筛选。",
  "home.sidebar.noArchived": "还没有归档任何会话。",
  "home.sidebar.noSessions": "还没有会话。",

  "home.empty.title": "开始本地转写",
  "home.empty.body":
    "转写记录按日期存在这里。在下方选好模型、语言和麦克风，再按“说话”。",

  "home.status.ready": "就绪",
  "home.status.listening": "聆听中",
  "home.status.transcribing": "转写中",
  "home.status.saved": "已保存",
  "home.status.noVoice": "未检测到人声 · 输入 {db}",

  "home.transcript.format": "整理",
  "home.transcript.formatting": "整理中",
  "home.transcript.redo": "重新整理",
  "home.transcript.formatTitle": "整理成 Markdown",
  "home.transcript.formatDisabled": "请先在设置里配置 LLM",
  "home.transcript.formatted": "已整理 · {preset}",
  "home.transcript.livePartial": "实时预览",

  "home.deck.talk": "说话",
  "home.deck.stop": "停止",
  "home.deck.model": "转写模型",
  "home.deck.language": "转写语言",
  "home.deck.microphone": "麦克风",
  "home.deck.microphoneDefault": "麦克风 · 默认为 {name}",
  "home.deck.microphoneNone": "默认麦克风",
  "home.deck.noMicrophone": "没有找到麦克风，请检查系统声音设置。",
  "home.deck.input": "输入",
  "home.deck.idle": "静默",

  // ---- Session search, filters and archive -------------------------------
  "search.placeholder": "搜索转写内容",
  "search.clear": "清除搜索",
  "search.filters": "筛选",
  "search.reset": "重置",
  "search.archive": "归档",
  "search.restore": "取消归档",
  "search.archiveTitle": "把“{title}”从主列表里收起。不会删除任何内容，转写照样能搜到，也随时可以恢复。",
  "search.restoreTitle": "把“{title}”移回主列表",
  "search.filterLanguage": "按语言筛选",
  "search.anyLanguage": "所有语言",
  "search.filterModel": "按模型筛选",
  "search.anyModel": "所有模型",
  "search.fromDate": "起始日期",
  "search.toDate": "结束日期",
  "search.dateSeparator": "至",
  "search.archiveScope": "归档范围",
  "search.scopeActive": "未归档",
  "search.scopeArchived": "已归档",
  "search.scopeAll": "全部",
  "search.archivedTag": "· 已归档",

  "search.matches": "{count} 条结果",
  "search.matchesOne": "{count} 条结果",
  "search.sessions": "{count} 个会话",
  "search.sessionsOne": "{count} 个会话",
  "search.summary": "在 {sessions}中找到 {matches}",
  "search.truncated": " · 仅显示最近的",

  "search.empty.title": "没有转写内容包含“{query}”",
  "search.empty.filters": "整个历史都搜过了，但筛选条件挡掉了一部分会话。放宽或清空筛选再试。",
  "search.empty.archived": "已归档的会话默认不参与搜索。把归档范围改成“{scope}”就会一起搜。",
  "search.empty.hint": "搜索会匹配转写里的任意连续片段，换个更短的词通常就能找到。",

  // ---- Settings window ---------------------------------------------------
  "settings.title": "设置",
  "settings.loading": "正在载入设置",
  "settings.reload": "重新载入设置",
  "settings.saved": "设置已保存。",

  // 分类栏。两个字一档，和右边的面板标题拉开层级；「服务」指的是云端转写和
  // Markdown 整理这两个外部接口，不叫「云端」是因为本地 Ollama 也走这里。
  "settings.nav": "设置分类",
  "settings.tab.general": "通用",
  "settings.tab.models": "模型",
  "settings.tab.services": "服务",
  "settings.tab.about": "关于",

  "settings.models.title": "模型",

  "settings.defaults.title": "默认设置",
  "settings.defaults.model": "模型",
  "settings.defaults.language": "语言",
  "settings.defaults.runtime": "运行时",
  "settings.defaults.uiLocale": "界面语言",

  "settings.cloud.title": "云端转写",
  "settings.cloud.descBefore": "可选。支持任何 OpenAI 兼容的 ",
  "settings.cloud.descAfter":
    " 接口。把转写模型选成 Cloud transcription 就会走云端；实时预览始终由本地模型负责。",
  "settings.cloud.presetLocal": "本地 (Ollama)",
  "settings.cloud.baseUrl": "Base URL",
  "settings.cloud.model": "模型",
  "settings.cloud.languageHint": "语言提示",
  "settings.cloud.languageHintPlaceholder": "留空则自动识别",
  "settings.cloud.apiKey": "API key",
  "settings.cloud.apiKeyPlaceholder": "留空则沿用已保存的 key",

  "settings.llm.title": "Markdown 整理",
  "settings.llm.desc":
    "可选。任何 OpenAI 兼容接口都可以，本地服务也行。原始转写一字不改地保留，整理后的版本单独存放。",
  "settings.llm.baseUrl": "Base URL",
  "settings.llm.model": "模型",
  "settings.llm.preset": "预设",
  "settings.llm.apiKey": "API key",
  "settings.llm.apiKeyStored": "API key（已保存）",
  "settings.llm.apiKeyKeep": "留空则不修改",
  "settings.llm.apiKeyOptional": "本地服务可留空",
  "settings.llm.apiKeyCleared": "API key 已清除。",
  "settings.llm.keychain": "API key 存在系统钥匙串里，不会写进应用数据库。",

  "settings.storage.title": "存储",
  "settings.storage.retainAudio": "保留录音文件",
  "settings.storage.autoPaste": "悬浮条自动粘贴",
  "settings.storage.liveInsert": "边说边输入",
  "settings.storage.liveInsertHint":
    "说完一句就送出一句，不再等整段说完一次性送出。更快，但更不准——短句的识别效果不如整段录音，而且字一旦进了别的程序就改不回来。",
  "settings.storage.save": "保存设置",

  "settings.hotkey.title": "按住说话快捷键",
  "settings.hotkey.desc": "在任何界面按住它录音，松开就转写。",
  "settings.hotkey.change": "更改",
  "settings.hotkey.recording": "按下新快捷键",
  "settings.hotkey.recordingHint": "修饰键加一个普通键，Esc 取消。",
  "settings.hotkey.reset": "恢复默认",
  "settings.hotkey.key.space": "空格",
  "settings.hotkey.needsModifier": "得配一个 Ctrl / Alt / Shift / Super，不然这个键在所有输入框里都按不出来了。",
  "settings.hotkey.unsupportedKey": "这个键不能用，换字母、数字、功能键或空格。",
  "settings.hotkey.applying": "正在应用",
  "settings.hotkey.live": "已生效，按住 {chord} 说话。",
  "settings.hotkey.notBound": "没在监听，按住不会有反应。",
  "settings.hotkey.conflict": "桌面环境保留了原来的绑定 {bound}，没有把这个快捷键交出来。请到系统设置的 {path} 里改。",
  "settings.hotkey.conflictPath": "快捷键 › Koushu 口述",
  "settings.hotkey.boundAs": "桌面环境报告的绑定是 {bound}。",
  "settings.hotkey.listener": "监听方式",
  "settings.hotkey.backend.portal": "桌面门户",
  "settings.hotkey.backend.evdev": "直接读键盘设备",
  "settings.hotkey.backend.ns-event": "macOS 事件监听",
  "settings.hotkey.backend.unavailable": "无",
  "settings.hotkey.portalNote": "门户快捷键归桌面环境管。第一次能在这里设，之后再改可能得去它自己的快捷键设置里。",

  "settings.trial.title": "试用",
  "settings.trial.licensed": "已激活授权。谢谢支持，这确实养活了这个项目。",
  "settings.trial.used": "{minutes} 分钟",
  "settings.trial.limit": "共 {minutes} 分钟",
  "settings.trial.note":
    "按 VAD 识别到的说话时长计算，不是按住快捷键的时长。额度用完后本地转写照常可用。",

  "settings.runtime.title": "运行时",
  "settings.runtime.ready": "内置运行时就绪",
  "settings.runtime.missing": "运行时缺失",
  "settings.runtime.engine": "引擎",
  "settings.runtime.compute": "计算",
  "settings.runtime.computeCpu": "仅 CPU",
  "settings.runtime.platform": "平台",

  "settings.paste.title": "Linux 粘贴",
  "settings.paste.session": "会话类型",
  "settings.paste.wayland": "Wayland",
  "settings.paste.x11": "X11",
  "settings.paste.tools": "工具",

  "settings.setup.title": "初始引导",
  "settings.setup.showAgain": "重新运行引导",
  "settings.setup.willShow": "下次打开主窗口时会重新显示引导。",

  // ---- First-run setup ---------------------------------------------------
  "setup.version": "Koushu 口述 {version}",
  "setup.headline": "欢迎使用本地语音转写",
  "setup.lede":
    "对着麦克风说话，本地即刻转成文字，记录只留在这台电脑上；用悬浮条可以把文字直接粘贴进任何应用。",

  "setup.feature.cpu.title": "纯 CPU 转写",
  "setup.feature.cpu.text": "内置官方 Fun-ASR llama.cpp 运行时，不需要 GPU、Python 或 CUDA。",
  "setup.feature.models.title": "Hugging Face 模型",
  "setup.feature.models.text": "首次设置时下载 Fun-ASR-Nano Q4_K。",
  "setup.feature.local.title": "转写留在本地",
  "setup.feature.local.text": "会话按日期存进本地 SQLite。",
  "setup.feature.private.title": "默认不联网",
  "setup.feature.private.text": "转写全程不需要任何云端服务。",

  "setup.card.title": "安装默认模型",
  "setup.card.subtitle": "来自 Hugging Face 的 Fun-ASR-Nano GGUF Q4_K",
  "setup.card.runtime": "运行时",
  "setup.card.runtimeBundled": "内置 CPU 运行时",
  "setup.card.model": "模型",
  "setup.card.download": "下载大小",
  "setup.card.downloadSize": "约 897 MB",
  "setup.card.status": "状态",
  "setup.card.clipboard": "剪贴板",
  "setup.card.clipboardFull": "可复制并粘贴",
  "setup.card.clipboardCopyOnly": "仅支持复制",
  "setup.card.note": "运行时已随应用打包。模型只需下载一次，存在应用数据目录里。",

  "setup.downloading": "正在从 Hugging Face 下载 Fun-ASR-Nano Q4_K GGUF 模型。",
  "setup.downloadingShort": "正在从 Hugging Face 下载 Fun-ASR-Nano",
  "setup.paused": "下载已暂停，随时可以继续。",
  "setup.continue": "继续",
  "setup.resume": "继续下载",
  "setup.downloadAndContinue": "下载模型并继续",
  "setup.skip": "暂不下载，先进应用",

  // ---- Voice bar ---------------------------------------------------------
  "bar.talk": "说话",
  "bar.stop": "停止",
  "bar.hide": "隐藏",
  "bar.noHotkey": "无快捷键",
  "bar.inputLevel": "输入电平",
  "bar.drag": "拖动可移动 · {anchor}",
  "bar.noSpeech": "没听到声音",
  "bar.noTranscript": "没有转写结果",
  "bar.sessionTitle": "语音 {time}",

  "anchor.top-left": "左上",
  "anchor.top-center": "上方居中",
  "anchor.top-right": "右上",
  "anchor.bottom-left": "左下",
  "anchor.bottom-center": "下方居中",
  "anchor.bottom-right": "右下",
};
