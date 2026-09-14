import { useOrganization } from "@clerk/clerk-react";
import { useEffect, useState } from "react";
import type { UsageSummary } from "@weldspeak/protocol";
import { useApi } from "../lib/useApi.js";
import { PRICING_PAGE } from "../marketing/links.js";

/** Format seconds the way someone thinks about dictation time. */
function humanMinutes(seconds: number): string {
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes} min`;
  const hours = Math.floor(minutes / 60);
  return `${hours}h ${minutes % 60}m`;
}

function humanWords(n: number): string {
  return n.toLocaleString("en-US");
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

  const wordCap = usage.monthlyWordCap;
  const wordFraction =
    wordCap != null && wordCap > 0 ? Math.min(usage.wordCount / wordCap, 1) : 0;

  const capSeconds = usage.monthlyMinuteCap ? usage.monthlyMinuteCap * 60 : null;
  const minuteFraction = capSeconds ? Math.min(usage.audioSeconds / capSeconds, 1) : 0;

  return (
    <section>
      <h2>Usage</h2>
      <p className="muted">
        Words and dictation time since {new Date(usage.periodStart).toLocaleDateString()}.
        {usage.entitlement === "free" ? (
          <>
            {" "}
            Free plan is {humanWords(wordCap ?? 2000)} words per month.{" "}
            <a href={PRICING_PAGE}>Subscribe for uncapped words</a>.
          </>
        ) : usage.entitlement === "weldsuite" ? (
          <> WeldSuite-included — uncapped words.</>
        ) : (
          <> Paid plan — uncapped words.</>
        )}
      </p>

      <div className="card">
        <h3>Your words this month</h3>
        <div className="usage-total">{humanWords(usage.wordCount)}</div>

        {wordCap != null ? (
          <>
            <div
              className="meter"
              role="img"
              aria-label={`${Math.round(wordFraction * 100)} percent of the monthly word limit used`}
            >
              <div className="meter-fill" style={{ width: `${wordFraction * 100}%` }} />
            </div>
            <p className="muted small">of {humanWords(wordCap)} word limit</p>
          </>
        ) : (
          <p className="muted small">No word limit on your plan.</p>
        )}
      </div>

      <div className="card">
        <h3>Org dictation time</h3>
        <div className="usage-total">{humanMinutes(usage.audioSeconds)}</div>

        {capSeconds ? (
          <>
            <div
              className="meter"
              role="img"
              aria-label={`${Math.round(minuteFraction * 100)} percent of the monthly minute limit used`}
            >
              <div className="meter-fill" style={{ width: `${minuteFraction * 100}%` }} />
            </div>
            <p className="muted small">of {usage.monthlyMinuteCap} min org limit</p>
          </>
        ) : (
          <p className="muted small">No org minute limit set.</p>
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
                <span className="muted">
                  {humanWords(row.wordCount)} words · {humanMinutes(row.audioSeconds)}
                </span>
              </li>
            ))}
          </ul>
        )}
        {!isAdmin && (
          <p className="muted small">Only admins can see a breakdown by member.</p>
        )}
      </div>
    </section>
  );
}
