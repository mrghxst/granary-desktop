import { useEffect } from "react";
import { HashRouter, Route, Routes, useNavigate } from "react-router";
import { goSignIn } from "./lib/nav";
import { onRevoked } from "./lib/silo";
import { AddServer } from "./routes/AddServer";
import { Home } from "./routes/Home";
import { Libraries } from "./routes/Libraries";
import { Login } from "./routes/Login";
import { Profiles } from "./routes/Profiles";

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

export function AppRoutes() {
  return (
    <>
      <RevokedListener />
      <Routes>
        <Route path="/servers/add" element={<AddServer />} />
        <Route path="/servers/:serverId/login" element={<Login />} />
        <Route path="/servers/:serverId/profiles" element={<Profiles />} />
        <Route path="/servers/:serverId/libraries" element={<Libraries />} />
        <Route path="*" element={<Home />} />
      </Routes>
    </>
  );
}

export function App() {
  return (
    <HashRouter>
      <AppRoutes />
    </HashRouter>
  );
}
