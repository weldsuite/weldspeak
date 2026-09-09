import {
  OrganizationSwitcher,
  SignedIn,
  SignedOut,
  SignIn,
  UserButton,
} from "@clerk/clerk-react";
import { NavLink, Navigate, Route, Routes } from "react-router-dom";
import { LinkDevice } from "./routes/LinkDevice.js";
import { Team } from "./routes/Team.js";
import { Dictionary } from "./routes/Dictionary.js";
import { Usage } from "./routes/Usage.js";
import { History } from "./routes/History.js";

export function App() {
  return (
    <>
      <SignedOut>
        <div className="centered">
          <div className="brand">
            <h1>WeldSpeak</h1>
            <p>Talk. It types.</p>
          </div>
          {/* Clerk's own component handles sign-in, sign-up and MFA. Building a
              custom flow here would be work with no product benefit. */}
          <SignIn routing="hash" />
        </div>
      </SignedOut>

      <SignedIn>
        <header className="topbar">
          <NavLink to="/" className="wordmark">
            WeldSpeak
          </NavLink>

          <nav>
            <NavLink to="/dictionary">Dictionary</NavLink>
            <NavLink to="/history">History</NavLink>
            <NavLink to="/team">Team</NavLink>
            <NavLink to="/usage">Usage</NavLink>
          </nav>

          <div className="topbar-right">
            {/* hidePersonal is deliberate: shared glossaries and usage caps only
                mean something inside an org, so people always have one active. */}
            <OrganizationSwitcher
              hidePersonal
              afterSelectOrganizationUrl="/dictionary"
              afterCreateOrganizationUrl="/team"
            />
            <UserButton />
          </div>
        </header>

        <main>
          <Routes>
            <Route path="/link" element={<LinkDevice />} />
            <Route path="/dictionary" element={<Dictionary />} />
            <Route path="/history" element={<History />} />
            <Route path="/team" element={<Team />} />
            <Route path="/usage" element={<Usage />} />
            <Route path="*" element={<Navigate to="/dictionary" replace />} />
          </Routes>
        </main>
      </SignedIn>
    </>
  );
}
