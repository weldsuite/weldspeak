import { useOrganization } from "@clerk/clerk-react";
import { useCallback, useEffect, useState, type FormEvent } from "react";
import type { DictionaryTerm } from "@weldspeak/protocol";
import { useApi } from "../lib/useApi.js";
import { ApiError } from "../lib/api.js";

/**
 * Vocabulary the recognizer and the cleanup pass both draw on.
 *
 * Personal terms are private; shared terms belong to the organization and only
 * admins can change them. The shared list is the high-value one for a team —
 * alloy designations, customer names and part numbers are exactly what generic
 * speech models mangle.
 */
export function Dictionary() {
  const api = useApi();
  const { organization, membership } = useOrganization();
  const isAdmin = membership?.role === "org:admin";

  const [terms, setTerms] = useState<DictionaryTerm[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const [draft, setDraft] = useState("");
  const [soundsLike, setSoundsLike] = useState("");
  const [scope, setScope] = useState<"user" | "org">("user");

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const { terms } = await api.listTerms();
      setTerms(terms);
      setError(null);
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "Could not load your dictionary.");
    } finally {
      setLoading(false);
    }
  }, [api]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  async function add(event: FormEvent) {
    event.preventDefault();
    const term = draft.trim();
    if (!term) return;

    try {
      await api.addTerm(term, scope, soundsLike.trim() || undefined);
      setDraft("");
      setSoundsLike("");
      await refresh();
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "Could not add that term.");
    }
  }

  async function remove(id: string) {
    try {
      await api.deleteTerm(id);
      setTerms((current) => current.filter((term) => term.id !== id));
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "Could not remove that term.");
    }
  }

  const personal = terms.filter((term) => term.scope === "user");
  const shared = terms.filter((term) => term.scope === "org");

  return (
    <section>
      <h2>Dictionary</h2>
      <p className="muted">
        Words WeldSpeak should get right — names, alloys, part numbers, anything a
        general speech model would guess at.
      </p>

      <form className="card add-term" onSubmit={add}>
        <input
          value={draft}
          onChange={(event) => setDraft(event.target.value)}
          placeholder="Inconel 625"
          aria-label="Term"
        />
        <input
          value={soundsLike}
          onChange={(event) => setSoundsLike(event.target.value)}
          placeholder="sounds like (optional)"
          aria-label="Sounds like"
        />
        <select
          value={scope}
          onChange={(event) => setScope(event.target.value as "user" | "org")}
          aria-label="Scope"
        >
          <option value="user">Just me</option>
          {/* Server-side the write is refused for non-admins regardless; this
              only keeps people from discovering that by being told no. */}
          <option value="org" disabled={!isAdmin}>
            {organization?.name ?? "Team"}
            {isAdmin ? "" : " (admins only)"}
          </option>
        </select>
        <button className="primary" type="submit">
          Add
        </button>
      </form>

      {error && <p className="error">{error}</p>}
      {loading && <p className="muted">Loading…</p>}

      <TermList
        title={`${organization?.name ?? "Team"} glossary`}
        subtitle={
          isAdmin
            ? "Everyone in your team gets these."
            : "Shared with your team. Admins can change these."
        }
        terms={shared}
        onRemove={isAdmin ? remove : undefined}
      />

      <TermList
        title="Your terms"
        subtitle="Private to you."
        terms={personal}
        onRemove={remove}
      />
    </section>
  );
}

function TermList({
  title,
  subtitle,
  terms,
  onRemove,
}: {
  title: string;
  subtitle: string;
  terms: DictionaryTerm[];
  onRemove?: (id: string) => void;
}) {
  return (
    <div className="card">
      <h3>{title}</h3>
      <p className="muted small">{subtitle}</p>

      {terms.length === 0 ? (
        <p className="muted">Nothing here yet.</p>
      ) : (
        <ul className="term-list">
          {terms.map((term) => (
            <li key={term.id}>
              <span className="term">{term.term}</span>
              {term.soundsLike && <span className="muted small">≈ {term.soundsLike}</span>}
              {onRemove && (
                <button
                  className="ghost"
                  onClick={() => onRemove(term.id)}
                  aria-label={`Remove ${term.term}`}
                >
                  Remove
                </button>
              )}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
