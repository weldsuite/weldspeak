import { useEffect, useState } from "react";
import type { TranscriptRecord } from "@weldspeak/protocol";
import { useApi } from "../lib/useApi.js";

/**
 * Dictation history.
 *
 * Scoped to the signed-in person: there is no view of a colleague's dictations,
 * for admins or anyone else. Empty is a legitimate state here — an admin may
 * have turned retention off for the whole team.
 */
export function History() {
  const api = useApi();
  const [transcripts, setTranscripts] = useState<TranscriptRecord[]>([]);
  const [query, setQuery] = useState("");
  const [copied, setCopied] = useState<string | null>(null);

  useEffect(() => {
    const timer = setTimeout(() => {
      api
        .listTranscripts(query || undefined)
        .then((result) => setTranscripts(result.transcripts))
        .catch(() => setTranscripts([]));
    }, 200);

    return () => clearTimeout(timer);
  }, [api, query]);

  async function copy(record: TranscriptRecord) {
    await navigator.clipboard.writeText(record.formatted);
    setCopied(record.id);
    setTimeout(() => setCopied(null), 1500);
  }

  return (
    <section>
      <h2>History</h2>

      <input
        className="search"
        value={query}
        onChange={(event) => setQuery(event.target.value)}
        placeholder="Search your dictations"
        aria-label="Search dictations"
      />

      {transcripts.length === 0 ? (
        <p className="muted">
          Nothing here. If your team has transcript history turned off, dictations are
          never stored.
        </p>
      ) : (
        <ul className="history">
          {transcripts.map((record) => (
            <li key={record.id} className="card">
              <p className="transcript">{record.formatted}</p>
              <div className="row muted small">
                <span>{new Date(record.createdAt).toLocaleString()}</span>
                {record.appName && <span>{record.appName}</span>}
                <span>{(record.durationMs / 1000).toFixed(1)}s</span>
                <button className="ghost" onClick={() => copy(record)}>
                  {copied === record.id ? "Copied" : "Copy"}
                </button>
              </div>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
