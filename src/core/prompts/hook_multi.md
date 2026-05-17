runai 推了多个相关 skill。**先让用户挑**（可单选也可多选，一句话问清），不要替用户做选择。

候选：

{CANDIDATES}

**用户挑了之后立即做下面三步，不要回"下一轮再注入"也不要等用户再发指令**：

1. 对**用户选中的每一个** skill 都跑一次 Bash 拿 SKILL.md（runai 同时记账）：
   `CLAUDE_SESSION_ID={SESSION_ID} runai recommend get <skill 名>`
   多个 skill 就跑多次，每个都拿一次 SKILL.md。
2. 回复首行写 `激活 skill: A, B, C`（多个用逗号分隔）让用户看到激活了哪些
3. 立即用所有 SKILL.md 内容**组合执行**用户原本的 prompt，不再问也不再确认

举例 1（单选）：用户原 prompt "启动安卓模拟器"，候选 emulator-launch / ktv-car-debug-suite，用户回 "用整套链路" → 跑 `runai recommend get ktv-car-debug-suite` → 按内容执行。

举例 2（多选）：用户回 "都要" / "两个一起" / "全选" → 跑两次 get 拿 emulator-launch + ktv-car-debug-suite 的 SKILL.md → 组合执行（互补 skill 协同更准）。

**不要限制用户只选一个**。runai 不限制激活数量，主 agent 可以同时挂载多个 skill 一起工作。

**不要做的事**：别调 `sm_enable` / `sm_install` / `runai enable` / `runai install` / 任何 "activate" 工具。`runai recommend get` 本身就是激活。即使 `sm_list` 显示 disabled 也无所谓。
