import { useState } from "react";
import { errorMessage, type UpdateInfo } from "../lib/backend";
import { t, translateMessage } from "../lib/i18n";

interface Props {
  enabled: boolean;
  info: UpdateInfo | null;
  onToggle: (enabled: boolean) => Promise<void>;
  onCheck: () => Promise<void>;
}

export function UpdateSettings({ enabled, info, onToggle, onCheck }: Props) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const run = async (action: () => Promise<void>) => {
    setBusy(true); setError(null);
    try { await action(); } catch (e) { setError(errorMessage(e)); } finally { setBusy(false); }
  };
  return <section className="appearance-settings" aria-labelledby="updates-title">
    <h3 id="updates-title" className="k-title">{t("Updates")}</h3>
    <div className="settings-row"><span>{t("Kernest {version}", { version: __APP_VERSION__ })}</span><span className="spacer" /><button type="button" className="btn btn-secondary" disabled={busy || !enabled} onClick={() => run(onCheck)}>{busy ? t("Checking…") : t("Check now")}</button></div>
    <label className="check-row"><input type="checkbox" checked={enabled} disabled={busy} onChange={e => { const checked = e.target.checked; run(() => onToggle(checked)); }} />{t("Automatically check for updates")}</label>
    <p className="k-caption muted">{t("When enabled, Kernest checks GitHub Releases at most once a day. It only shows a notice and never installs updates automatically.")}</p>
    {!enabled && <p className="notice k-caption">{t("Update checking is off. No update requests are sent.")}</p>}
    {enabled && info?.latestVersion && <p role="status" className="notice k-label">{info.available ? t("Kernest {version} is available.", { version: info.latestVersion }) : t("You have the latest stable version.")}</p>}
    {enabled && info?.checkedAt && <p className="k-caption muted">{t("Last checked: {time}", { time: new Date(info.checkedAt * 1000).toLocaleString(document.documentElement.lang) })}</p>}
    {(error || (enabled && info?.error)) && <p role="alert" className="notice notice-warning">{error ?? translateMessage(info!.error!)}</p>}
  </section>;
}
