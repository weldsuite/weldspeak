import { OrganizationProfile, useOrganization } from "@clerk/clerk-react";
import { useEffect, useState } from "react";
import type { OrgSettings } from "@weldspeak/protocol";
import { useApi } from "../lib/useApi.js";

/**
 * Team management.
 *
 * Members, roles and invitations are Clerk's `<OrganizationProfile>` — that is
 * a complete invite flow, email delivery included, for no code. What Clerk does
 * not know about is WeldSpeak's own org policy, so that sits alongside it.
 */
export function Team() {
  const { organization, membership } = useOrganization();
  const api = useApi();
  const isAdmin = membership?.role === "org:admin";

  const [settings, setSettings] = useState<OrgSettings | null>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!organization) return;
    api
      .orgSettings()
      .then(setSettings)
      .catch(() => setError("Could not load team settings."));
  }, [api, organization?.id]);

  async function patch(change: Partial<OrgSettings>) {
    setSaving(true);
    setError(null);
    try {
      setSettings(await api.updateOrgSettings(change));
    } catch {
      setError("Could not save that change.");
    } finally {
      setSaving(false);
    }
  }

  if (!organization) {
    return <p className="muted">Select or create a team to manage it.</p>;
  }

  return (
    <section>
      <h2>{organization.name}</h2>

      <div className="card">
        <h3>Dictation policy</h3>

        {settings ? (
          <>
            <label className="setting">
              <input
                type="checkbox"
                checked={settings.retainTranscripts}
                disabled={!isAdmin || saving}
                onChange={(event) => patch({ retainTranscripts: event.target.checked })}
              />
              <span>
                <strong>Keep transcript history</strong>
                <span className="muted small">
                  When off, dictations are transcribed and discarded — no copy is kept
                  on the server or on anyone&rsquo;s machine.
                </span>
              </span>
            </label>

            <label className="setting">
              <span>
                <strong>Monthly limit</strong>
                <span className="muted small">
                  Minutes of dictation per month across the team. Leave empty for no
                  limit.
                </span>
              </span>
              <input
                type="number"
                min={0}
                value={settings.monthlyMinuteCap ?? ""}
                disabled={!isAdmin || saving}
                placeholder="No limit"
                onChange={(event) =>
                  patch({
                    monthlyMinuteCap:
                      event.target.value === "" ? null : Number(event.target.value),
                  })
                }
              />
            </label>

            {!isAdmin && (
              <p className="muted small">Only team admins can change these.</p>
            )}
            {error && <p className="error">{error}</p>}
          </>
        ) : (
          <p className="muted">Loading…</p>
        )}
      </div>

      <div className="card">
        <h3>Members</h3>
        <OrganizationProfile routing="hash" />
      </div>
    </section>
  );
}
