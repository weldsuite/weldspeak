import { useOrganization } from "@clerk/clerk-react";
import { useEffect, useState } from "react";
import { useSearchParams } from "react-router-dom";
import { useApi } from "../lib/useApi.js";
import { ApiError } from "../lib/api.js";

type State =
  | { status: "confirming" }
  | { status: "working" }
  | { status: "approved"; label: string }
  | { status: "denied" }
  | { status: "error"; message: string };

/**
 * Device approval — the step that binds a Clerk identity to a desktop install.
 *
 * The desktop app sends the user here with a short code, then polls. Approving
 * is a deliberate button press rather than something that happens on page load:
 * this is the one moment a person can notice that a code they did not initiate
 * is waiting, and stop it.
 */
export function LinkDevice() {
  const [params] = useSearchParams();
  const api = useApi();
  const { organization } = useOrganization();

  const [code, setCode] = useState(params.get("code")?.toUpperCase() ?? "");
  const [state, setState] = useState<State>({ status: "confirming" });

  useEffect(() => {
    const fromUrl = params.get("code");
    if (fromUrl) setCode(fromUrl.toUpperCase());
  }, [params]);

  async function approve() {
    setState({ status: "working" });
    try {
      const result = await api.approveDevice(code.trim());
      setState({ status: "approved", label: result.device.label });
    } catch (error) {
      setState({
        status: "error",
        message:
          error instanceof ApiError
            ? error.message
            : "Something went wrong. Try again from the app.",
      });
    }
  }

  async function deny() {
    setState({ status: "working" });
    try {
      await api.denyDevice(code.trim());
    } catch {
      // The desktop grant expires on its own within ten minutes, so a failed
      // denial is not worth alarming the user about.
    }
    setState({ status: "denied" });
  }

  if (state.status === "approved") {
    return (
      <section className="card centered-card">
        <h2>{state.label} is connected</h2>
        <p>You can close this tab and go back to the app.</p>
        {organization && (
          <p className="muted">
            Dictations will use <strong>{organization.name}</strong>&rsquo;s shared glossary.
          </p>
        )}
      </section>
    );
  }

  if (state.status === "denied") {
    return (
      <section className="card centered-card">
        <h2>Request denied</h2>
        <p>That device was not connected.</p>
      </section>
    );
  }

  return (
    <section className="card centered-card">
      <h2>Connect a device</h2>
      <p>Check that this code matches the one shown in the WeldSpeak app.</p>

      <input
        className="code-input"
        value={code}
        onChange={(event) => setCode(event.target.value.toUpperCase())}
        placeholder="WXYZ-1234"
        spellCheck={false}
        autoComplete="off"
        aria-label="Device code"
      />

      {state.status === "error" && <p className="error">{state.message}</p>}

      <div className="row">
        <button
          className="primary"
          onClick={approve}
          disabled={state.status === "working" || code.trim().length < 8}
        >
          {state.status === "working" ? "Connecting…" : "Connect"}
        </button>
        <button onClick={deny} disabled={state.status === "working"}>
          This wasn&rsquo;t me
        </button>
      </div>

      <p className="muted small">
        Only connect a device if you just started sign-in from the WeldSpeak app on
        that machine.
      </p>
    </section>
  );
}
