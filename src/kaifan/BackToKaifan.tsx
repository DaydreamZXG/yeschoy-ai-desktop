import { useTranslation } from "react-i18next";
import { openKaifan } from "../uiChoice";

/**
 * 旧界面右下角的「回到首页」。旧界面本身一行不改，只在它旁边挂这一个按钮；
 * 样式写在这里，不依赖任何一套样式表。
 */
export function BackToKaifan() {
  const { t } = useTranslation();
  return (
    <button
      type="button"
      data-testid="back-to-kaifan"
      onClick={openKaifan}
      style={{
        position: "fixed",
        right: 20,
        bottom: 20,
        zIndex: 2147483000,
        height: 40,
        padding: "0 18px",
        borderRadius: 20,
        border: "1px solid rgba(255,255,255,.28)",
        background: "#101a3f",
        color: "#fff",
        font: "600 13px system-ui, sans-serif",
        boxShadow: "0 10px 30px rgba(0,0,0,.35)",
        cursor: "pointer",
      }}
    >
      {t("kaifan.backHome")}
    </button>
  );
}
