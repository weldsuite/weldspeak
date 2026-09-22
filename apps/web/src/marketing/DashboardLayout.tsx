import { OrganizationSwitcher, UserButton, useAuth } from "@clerk/clerk-react";
import { useEffect, useState } from "react";
import { NavLink, Navigate, Outlet, useLocation } from "react-router-dom";
import { Brand } from "@/marketing/Brand";

export function DashboardLayout() {
  const { isLoaded, isSignedIn } = useAuth();
  const location = useLocation();
  const [waited, setWaited] = useState(false);
  const redirect = `/sign-in?redirect_url=${encodeURIComponent(location.pathname + location.search)}`;

  useEffect(() => {
    const timer = window.setTimeout(() => setWaited(true), 2500);
    return () => window.clearTimeout(timer);
  }, []);

  if (isSignedIn) {
    return (
      <div data-app="dashboard">
        <header className="topbar">
          <Brand />

          <nav>
            <NavLink to="/dictionary">Dictionary</NavLink>
            <NavLink to="/history">History</NavLink>
            <NavLink to="/team">Team</NavLink>
            <NavLink to="/usage">Usage</NavLink>
          </nav>

          <div className="topbar-right">
            <OrganizationSwitcher
              hidePersonal
              afterSelectOrganizationUrl="/dictionary"
              afterCreateOrganizationUrl="/team"
            />
            <UserButton />
          </div>
        </header>

        <main>
          <Outlet />
        </main>
      </div>
    );
  }

  if (!isLoaded && !waited) {
    return (
      <div data-app="dashboard" className="flex min-h-svh items-center justify-center">
        <p className="muted">Loading…</p>
      </div>
    );
  }

  return <Navigate to={redirect} replace />;
}
