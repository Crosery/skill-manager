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
之后：每行一个 skill name，最多 {TOP_K} 个，第一行最相关。
完全不相关：第一行 `EXCLUSIVE`，下面无 skill。
