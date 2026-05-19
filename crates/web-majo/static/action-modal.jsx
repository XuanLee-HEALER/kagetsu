// Action Modal — floating overlay listing available actions (riichi / tsumo / ankan / discard / tsumogiri).

function ActionModal() {
  return (
    <>
      {/* scrim */}
      <div style={{
        position: "absolute", inset: 0,
        background: "rgba(12,12,20,0.65)",
        backdropFilter: "blur(20px) saturate(140%)",
        zIndex: 50,
      }} />
      {/* modal */}
      <div style={{
        position: "absolute", left: "50%", top: "44%",
        transform: "translate(-50%, -50%)",
        width: 520,
        background: "var(--bg-deep)",
        border: "1px solid var(--border-strong)",
        borderRadius: "var(--radius-xl)",
        boxShadow: "var(--shadow-4)",
        padding: "var(--space-6)",
        zIndex: 51,
      }}>
        <div style={{
          display: "flex", alignItems: "baseline", justifyContent: "space-between",
          marginBottom: 16,
        }}>
          <div>
            <Eyebrow>Action · 行动</Eyebrow>
            <div style={{
              fontFamily: "var(--font-serif)", fontSize: "var(--text-xl)",
              marginTop: 4, letterSpacing: "var(--tracking-tight)",
            }}>
              你摸到 <span style={{ color: PIG.gser }}>七筒</span> · 已聴牌
            </div>
          </div>
          <KeyBadge k="Esc" label="关闭" size="sm" />
        </div>

        <div style={{ display: "flex", flexDirection: "column", gap: 2 }}>
          <ActionOption k="R" label="立直 · Riichi" tone="primary" selected
            detail="切 四筒  → 聴 三筒 六筒 ・ 見込打点 5,200 / 7,700" />
          <ActionOption k="W" label="自摸 · Tsumo" disabled
            detail="无満和牌役（缺自摸役）" />
          <ActionOption k="K" label="暗杠 · Ankan" disabled
            detail="无可暗杠组" />
          <ActionOption k="D" label="切牌 · Discard"
            detail="选择手牌中一张打出（默认聴牌切）" />
          <ActionOption k="T" label="摸切 · Tsumogiri"
            detail="切出刚摸到的 七筒（不变手牌）" />
          <ActionOption k="9" label="九种九牌 · Kyuushuu" disabled
            detail="非第一巡 · 不可" />
        </div>

        <div style={{
          marginTop: 18, paddingTop: 14,
          borderTop: "1px solid var(--border-subtle)",
          display: "flex", justifyContent: "space-between", alignItems: "center",
          color: "var(--fg-tertiary)", fontSize: 12,
        }}>
          <span>
            <KeyBadge k="↑↓" size="sm" />
            <span style={{ marginLeft: 8 }}>选择 ·</span>
            <span style={{ marginLeft: 8 }}>
              <KeyBadge k="Enter" size="sm" label="确认" />
            </span>
          </span>
          <span style={{ fontFamily: "var(--font-mono)" }}>
            或直接按字母键跳过此菜单
          </span>
        </div>
      </div>
    </>
  );
}

function ActionOption({ k, label, detail, tone, selected, disabled }) {
  return (
    <div style={{
      display: "grid",
      gridTemplateColumns: "auto 140px 1fr",
      alignItems: "center", gap: 14,
      padding: "10px 12px",
      background: selected ? "var(--accent-soft)" : "transparent",
      border: `1px solid ${selected ? "var(--border-focus)" : "transparent"}`,
      borderRadius: "var(--radius-md)",
      opacity: disabled ? 0.45 : 1,
    }}>
      <KeyBadge k={k} tone={selected ? "primary" : tone || "default"} />
      <span style={{
        fontWeight: 500,
        color: disabled ? "var(--fg-disabled)" : selected ? "var(--fg-primary)" : "var(--fg-primary)",
        fontSize: 14,
      }}>{label}</span>
      <span style={{
        color: "var(--fg-tertiary)", fontSize: 12,
      }}>{detail}</span>
    </div>
  );
}

window.ActionModal = ActionModal;
