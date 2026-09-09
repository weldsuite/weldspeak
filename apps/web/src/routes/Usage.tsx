import { useOrganization } from "@clerk/clerk-react";
import { useEffect, useState } from "react";
import type { UsageSummary } from "@weldspeak/protocol";
import { useApi } from "../lib/useApi.js";

/** Format seconds the way someone thinks about dictation time. */
function humanMinutes(seconds: number): string {
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes} min`;
  const hours = Math.floor(minutes / 60);
  return `${hours}h ${minutes % 60}m`;
}

export function Usage() {
  const api = useApi();
  const { organization, membership } = useOrganization();
  const isAdmin = membership?.role === "org:admin";

  const [usage, setUsage] = useState<UsageSummary | null>(null);

  useEffect(() => {
    api.usage().then(setUsage).catch(() => setUsage(null));
  }, [api, organization?.id]);

  if (!usage) return <p className="muted">Loading…</p>;

  const capSeconds = usage.monthlyMinuteCap ? usage.monthlyMinuteCap * 60 : null;
  const fraction = capSeconds ? Math.min(usage.audioSeconds / capSeconds, 1) : 0;

  return (
    <section>
      <h2>Usage</h2>
      <p className="muted">
        Dictation time since {new Date(usage.periodStart).toLocaleDateString()}.
      </p>

      <div className="card">
        <div className="usage-total">{humanMinutes(usage.audioSeconds)}</div>

        {capSeconds ? (
          <>
            <div className="meter" role="img"
                 aria-label={`${Math.round(fraction * 100)} percent of the monthly limit used`}>
              <div className="meter-fill" style={{ width: `${fraction * 100}%` }} />
            </div>
            <p className="muted small">
              of {usage.monthlyMinuteCap} min limit
            </p>
          </>
        ) : (
          <p className="muted small">No monthly limit set.</p>
        )}
      </div>

      <div className="card">
        <h3>{isAdmin ? "By member" : "Your usage"}</h3>
        {usage.byUser.length === 0 ? (
          <p className="muted">No dictations yet this month.</p>
        ) : (
          <ul className="term-list">
            {usage.byUser.map((row) => (
              <li key={row.userId}>
                <span className="term">{row.userId}</span>
                <span className="muted">{humanMinutes(row.audioSeconds)}</span>
              </li>
            ))}
          </ul>
        )}
        {!isAdmin && (
          <p className="muted small">
            Only admins can see a breakdown by member.
          </p>
        )}
      </div>
    </section>
  );
}
