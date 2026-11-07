import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

/** A thin banner while Windows reports no network; pages keep what they loaded and retry on their own. */
export function OfflineBanner() {
  const { t } = useTranslation();
  const [online, setOnline] = useState(() => (typeof navigator === "undefined" ? true : navigator.onLine));
  useEffect(() => {
    const up = () => {
      setOnline(true);
    };
    const down = () => {
      setOnline(false);
    };
    window.addEventListener("online", up);
    window.addEventListener("offline", down);
    return () => {
      window.removeEventListener("online", up);
      window.removeEventListener("offline", down);
    };
  }, []);
  if (online) return null;
  return (
    <div
      role="status"
      aria-live="polite"
      data-testid="offline"
      className="fixed inset-x-0 top-0 z-40 bg-amber-600 px-4 py-1.5 text-center text-sm font-medium text-white"
    >
      {t("status.offline")}
    </div>
  );
}
