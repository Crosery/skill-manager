runai 推荐 skill：**{NAME}** — {DESC}

对口就跑下面这条 Bash 一次性拿 SKILL.md 内容（runai 同时自动 +1 usage_count + 当前 session 不再重推）：

```
CLAUDE_SESSION_ID={SESSION_ID} runai recommend get {NAME}
```

stdout 就是 SKILL.md 全文，按内容执行；回复首行写 `激活 skill: {NAME}`。不对口就忽略不要跑命令。**不要**用 Read 工具自己找 SKILL.md 路径 —— runai 不再公开路径，只暴露这一条命令；不要调 sm_enable / sm_install / runai install。
