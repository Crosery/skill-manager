## 用户当前 prompt (最高优先级，必须先看这段判断意图)

```
{USER_PROMPT}
```

{CWD_BLOCK}{PROJECT_CONTEXT_BLOCK}{HISTORY_BLOCK}{ALREADY_ROUTED_BLOCK}候选 skill:
{CANDIDATE_LISTING}

---

回到用户当前 prompt 做最终判断：

```
{USER_PROMPT}
```

输出格式（严格）：
第一行：`COMPATIBLE` 或 `EXCLUSIVE`
第二行（可选但强烈建议）：`reasoning: <一句话，用户在做什么 + 为什么推这套 skill 组合>`
之后：每行一个 skill name，第一行最相关。

候选数量自由判断：COMPATIBLE 型工作流可以推多个互补 skill（emulator + adb + cdp + figma 4 个都给），EXCLUSIVE 型挑 1-3 个让用户选，硬上限 {TOP_K}。按工作流实际需要数量推，不要为凑数也不要漏配套。

完全不相关：第一行 `EXCLUSIVE`，下面无 skill。
