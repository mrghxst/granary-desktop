import { useEffect, type ReactNode } from "react";
import { HashRouter, Navigate, Route, Routes, useNavigate } from "react-router";
import { goSignIn } from "./lib/nav";
import { onRevoked } from "./lib/silo";
import { AddServer } from "./routes/AddServer";
import { Home } from "./routes/Home";
import { Shell } from "./components/Shell";
import { applySettings, loadSettings } from "./lib/settings";
import { Item } from "./routes/Item";
import { Library } from "./routes/Library";
import { Search } from "./routes/Search";
import { ServerHome } from "./routes/ServerHome";
import { Settings } from "./routes/Settings";
import { Login } from "./routes/Login";
import { Profiles } from "./routes/Profiles";
import { usePlayer } from "./player/context";
import { PlayerProvider } from "./player/PlayerProvider";
import { PlayerView } from "./player/PlayerView";

/** The host emits `auth://revoked` when the server rejects a stored session: go back to sign-in for that server. */
function RevokedListener() {
  const navigate = useNavigate();
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let alive = true;
    void onRevoked((serverId) => {
      goSignIn(navigate, serverId);
    }).then((u) => {
      if (alive) unlisten = u;
      else u();
    });
    return () => {
      alive = false;
      unlisten?.();
    };
  }, [navigate]);
  return null;
}

/** The library stays mounted (scroll, state) but hidden while the player view is up. */
function HiddenWhilePlaying({ children }: { children: ReactNode }) {
  const { state } = usePlayer();
  return <div className={state.active ? "hidden" : "contents"}>{children}</div>;
}

export function AppRoutes() {
  return (
    <PlayerProvider>
      <RevokedListener />
      <PlayerView />
      <HiddenWhilePlaying>
        <Routes>
          <Route path="/servers/add" element={<AddServer />} />
          <Route path="/servers/:serverId/login" element={<Login />} />
          <Route path="/servers/:serverId/profiles" element={<Profiles />} />
          <Route path="/servers/:serverId" element={<Shell />}>
            <Route path="home" element={<ServerHome />} />
            <Route path="library/:libraryId" element={<Library />} />
            <Route path="search" element={<Search />} />
            <Route path="item/:itemId" element={<Item />} />
            <Route path="settings" element={<Settings />} />
            <Route index element={<Navigate to="home" replace />} />
            <Route path="libraries" element={<Navigate to="../home" replace />} />
          </Route>
          <Route
            path="/settings"
            element={
              <main className="mx-auto w-full max-w-3xl p-6">
                <Settings />
              </main>
            }
          />
          <Route path="*" element={<Home />} />
        </Routes>
      </HiddenWhilePlaying>
    </PlayerProvider>
  );
}

/** Theme and language from the saved settings, once at startup (the Settings screen re-applies on change). */
function ApplySettings() {
  useEffect(() => {
    void loadSettings()
      .then(applySettings)
      .catch(() => undefined);
  }, []);
  return null;
}

export function App() {
  return (
    <HashRouter>
      <ApplySettings />
      <AppRoutes />
    </HashRouter>
  );
}
