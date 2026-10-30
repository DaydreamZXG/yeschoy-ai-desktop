import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Toaster, toast } from "sonner";
import { WorkbenchSidebar, type AppView } from "./workbench/WorkbenchChrome";
import { refreshModelCatalog } from "./model-profiles/remoteCatalog";
import { useAppearance } from "./workbench/appearance";
import { AccountView } from "./workbench/AccountView";
import { ModelsView } from "./workbench/ModelsView";
import { AppLibraryView } from "./workbench/AppLibraryView";
import { ConfigurationPreviewView } from "./configuration/ConfigurationPreviewView";
import type { ActivationToolId } from "./configuration/activation";
import type { SetupIntent } from "./configuration/setupIntent";
import {
  ConnectionProvider,
  useToolConnections,
} from "./configuration/connections";
import { DiagnosticsView } from "./diagnostics/DiagnosticsView";
import { FeedbackView } from "./diagnostics/FeedbackView";
import { SettingsView } from "./settings/SettingsView";
import {
  AnnouncementBanner,
  AnnouncementsView,
} from "./workbench/AnnouncementsView";
import { CommunityGroupDialog } from "./workbench/QQGroupDialog";
import {
  followAnnouncement,
  readAnnouncements,
  readSeenAnnouncements,
  rememberSeenAnnouncements,
  type Notice,
} from "./workbench/announcements";
import { useWorkbenchCopy } from "./workbench/copy";
import { ShutdownProvider } from "./settings/QuitAssistant";
import { InstallationProvider } from "./installation/InstallationProvider";
import { InstallationNotice } from "./installation/InstallationPanel";
import { UpdateProvider } from "./update/UpdateProvider";
import { UpdateNotice } from "./update/UpdateNotice";
import {
  catalogLineFromSnapshot,
  initialAccountLineId,
  readAccountLinePreference,
  writeAccountLinePreference,
} from "./account/accountLinePreference";
import { useAccountSession } from "./account/useAccountSession";
import { StationTrustDialog } from "./account/StationTrustDialog";
import { useStationConnect } from "./account/useStationConnect";
import { isAccountLineId, type AccountLineId } from "./configuration/preview";

const ANNOUNCEMENT_REFRESH_MS = 30 * 60 * 1000;
const ANNOUNCEMENT_FOCUS_REFRESH_MS = 5 * 60 * 1000;

function App() {
  const [view, setView] = useState<AppView>("home");
  const [selectedDesktopApp, setSelectedDesktopApp] =
    useState<ActivationToolId>("claude_desktop");
  const [setupIntent, setSetupIntent] = useState<SetupIntent>();
  const connections = useToolConnections();
  const [setupVisited, setSetupVisited] = useState(false);
  useEffect(() => {
    if (view === "setup") setSetupVisited(true);
  }, [view]);
  // M5: try a remote model-catalog revision in the background; any failure
  // silently keeps the bundled catalog and never blocks startup.
  useEffect(() => {
    void refreshModelCatalog().catch(() => undefined);
  }, []);
  const [accountLineId, setAccountLineId] =
    useState<AccountLineId>(initialAccountLineId);
  const applyAccountLine = useCallback((lineId: AccountLineId) => {
    setAccountLineId(lineId);
    if (lineId === "mainland_optimized" || lineId === "global_accelerated") {
      writeAccountLinePreference(lineId);
    }
  }, []);
  const [communityOpen, setCommunityOpen] = useState(false);
  const [announcements, setAnnouncements] = useState<{
    available: boolean;
    notices: Notice[];
    loading: boolean;
    failed: boolean;
  }>({ available: false, notices: [], loading: false, failed: false });
  const accountSession = useAccountSession(accountLineId, {
    onSessionSnapshot: (snapshot) => {
      const fromSession = catalogLineFromSnapshot(snapshot.storedLineId);
      if (fromSession) {
        applyAccountLine(fromSession);
        return;
      }
      const preference = readAccountLinePreference();
      if (preference && isAccountLineId(preference)) {
        setAccountLineId(preference);
      }
    },
  });
  useStationConnect({
    lineId: accountLineId,
    onLineChange: applyAccountLine,
    refresh: accountSession.refresh,
  });
  const shellRef = useRef<HTMLElement>(null);
  const previousView = useRef(view);
  const { appearance, changeAppearance } = useAppearance();
  useEffect(() => {
    window.scrollTo({ top: 0, behavior: "instant" });
    if (previousView.current === view) return;
    previousView.current = view;
    const heading = Array.from(
      shellRef.current?.querySelectorAll("h1") ?? [],
    ).find((candidate) => !candidate.closest("[hidden]"));
    if (heading) {
      heading.tabIndex = -1;
      heading.focus({ preventScroll: true });
    }
  }, [view]);

  // 公告登录后读一次，之后每 30 分钟、以及窗口回到前台（距上次超过
  // 5 分钟）时再读 —— 助手常常一开一整天，活动公告不能等到下次启动。
  // 读不到只是没有公告，绝不能因为它让主界面转圈或报错。
  // 入口本身也只在服务端宣告了地址时才出现。
  const signedIn = accountSession.projection?.status === "signed_in";
  const announcementsReadAt = useRef(0);
  const loadAnnouncements = useCallback(async () => {
    announcementsReadAt.current = Date.now();
    setAnnouncements((current) => ({
      ...current,
      loading: true,
      failed: false,
    }));
    const result = await readAnnouncements(accountLineId);
    setAnnouncements((current) => ({
      // 一旦确认服务端支持公告，入口就不再消失。否则一次网络抖动会让
      // 侧边栏少一项，用户会以为自己点错了地方。读失败改成显示重试。
      available: result.available || current.available,
      notices: result.available ? result.notices : current.notices,
      loading: false,
      failed: !result.available && current.available,
    }));
  }, [accountLineId]);
  useEffect(() => {
    if (!signedIn) {
      setAnnouncements({
        available: false,
        notices: [],
        loading: false,
        failed: false,
      });
      return;
    }
    void loadAnnouncements();
    const timer = window.setInterval(
      () => void loadAnnouncements(),
      ANNOUNCEMENT_REFRESH_MS,
    );
    const onVisible = () => {
      if (
        document.visibilityState === "visible" &&
        Date.now() - announcementsReadAt.current > ANNOUNCEMENT_FOCUS_REFRESH_MS
      )
        void loadAnnouncements();
    };
    document.addEventListener("visibilitychange", onVisible);
    return () => {
      window.clearInterval(timer);
      document.removeEventListener("visibilitychange", onVisible);
    };
  }, [signedIn, loadAnnouncements]);

  const copy = useWorkbenchCopy();
  const [seenAnnouncements, setSeenAnnouncements] = useState(
    readSeenAnnouncements,
  );
  const unreadAnnouncements = useMemo(
    () =>
      new Set(
        announcements.notices
          .map((notice) => notice.id)
          .filter((id) => !seenAnnouncements.has(id)),
      ),
    [announcements.notices, seenAnnouncements],
  );
  const markAnnouncementsSeen = useCallback((ids: string[]) => {
    setSeenAnnouncements((current) =>
      ids.every((id) => current.has(id))
        ? current
        : rememberSeenAnnouncements(current, ids),
    );
  }, []);
  // 打开公告页就算全部读过；页面开着时新到的也一样。
  useEffect(() => {
    if (view === "announcements" && unreadAnnouncements.size > 0)
      markAnnouncementsSeen([...unreadAnnouncements]);
  }, [view, unreadAnnouncements, markAnnouncementsSeen]);
  const followNotice = useCallback(
    async (notice: Notice) => {
      markAnnouncementsSeen([notice.id]);
      if (!(await followAnnouncement(accountLineId, notice.id)))
        toast.error(copy.announcementActionFailed);
    },
    [accountLineId, copy.announcementActionFailed, markAnnouncementsSeen],
  );
  const bannerNotice = announcements.notices.find(
    (notice) => notice.banner && unreadAnnouncements.has(notice.id),
  );

  return (
    <UpdateProvider>
      <InstallationProvider>
        <ConnectionProvider value={connections}>
          <ShutdownProvider>
            <main ref={shellRef} className="app-shell" data-view={view}>
              <StationTrustDialog />
              <WorkbenchSidebar
                view={view}
                onNavigate={setView}
                onOpenCommunity={() => setCommunityOpen(true)}
                announcementsAvailable={announcements.available}
                announcementsUnread={unreadAnnouncements.size}
                appearance={appearance}
                onAppearance={changeAppearance}
                accountProjection={accountSession.projection}
                accountLoading={
                  accountSession.loading ||
                  (accountSession.stationCheck ?? "idle") === "checking"
                }
                accountLineId={accountLineId}
              />

              {(view === "setup" || setupVisited) && (
                <div className="persistent-setup" hidden={view !== "setup"}>
                  <ConfigurationPreviewView
                    initialDesktopAppId={selectedDesktopApp}
                    setupIntent={setupIntent}
                    enableLocalActivation
                    active={view === "setup"}
                    lineId={accountLineId}
                    onLineChange={applyAccountLine}
                    session={accountSession}
                    onOpenAccount={() => setView("account")}
                    onOpenHome={() => setView("home")}
                  />
                </div>
              )}
              {view === "home" ? (
                <AppLibraryView
                  onOpenCommunity={() => setCommunityOpen(true)}
                  onOpenAccount={() => setView("account")}
                  onStationLineChange={applyAccountLine}
                  accountLineId={accountLineId}
                  onOpenSetup={(appId, action = "configure") => {
                    setSelectedDesktopApp(appId);
                    setSetupIntent((previous) =>
                      previous?.appId === appId && previous.action === action
                        ? previous
                        : {
                            appId,
                            action,
                            revision: (previous?.revision ?? 0) + 1,
                          },
                    );
                    setView("setup");
                  }}
                  accountSession={accountSession}
                  announcement={
                    bannerNotice && (
                      <AnnouncementBanner
                        notice={bannerNotice}
                        onFollow={(notice) => void followNotice(notice)}
                        onOpenAll={() => setView("announcements")}
                        onDismiss={(notice) =>
                          markAnnouncementsSeen([notice.id])
                        }
                      />
                    )
                  }
                />
              ) : view === "account" ? (
                <AccountView
                  lineId={accountLineId}
                  onLineChange={setAccountLineId}
                  onOpenAppsHome={() => setView("home")}
                  session={accountSession}
                />
              ) : view === "models" ? (
                <ModelsView
                  line={accountLineId}
                  onLineChange={setAccountLineId}
                  session={accountSession}
                  onOpenAccount={() => setView("account")}
                />
              ) : view === "setup" ? null : view === "diagnostics" ? (
                <DiagnosticsView
                  onOpenSetup={() => setView("setup")}
                  onOpenAccount={() => setView("account")}
                  onOpenHome={() => setView("home")}
                  onReportBug={() => setView("feedback")}
                />
              ) : view === "feedback" ? (
                <FeedbackView
                  onOpenDiagnostics={() => setView("diagnostics")}
                  onOpenHome={() => setView("home")}
                />
              ) : view === "announcements" ? (
                <AnnouncementsView
                  notices={announcements.notices}
                  loading={announcements.loading}
                  failed={announcements.failed}
                  onRetry={() => void loadAnnouncements()}
                  unread={unreadAnnouncements}
                  onFollow={(notice) => void followNotice(notice)}
                />
              ) : view === "settings" ? (
                <SettingsView
                  appearance={appearance}
                  onAppearanceChange={changeAppearance}
                  onOpenAccount={() => setView("account")}
                  onOpenDiagnostics={() => setView("diagnostics")}
                  onOpenFeedback={() => setView("feedback")}
                  onOpenModels={() => setView("models")}
                />
              ) : null}
            </main>
            <div className="desktop-notice-stack">
              <UpdateNotice onOpenSettings={() => setView("settings")} />
              {view !== "setup" && (
                <InstallationNotice
                  onOpen={(tool) => {
                    setSelectedDesktopApp(tool);
                    setView("setup");
                  }}
                />
              )}
            </div>
            <CommunityGroupDialog
              open={communityOpen}
              onOpenChange={setCommunityOpen}
              lineId={accountLineId}
              accountProjection={accountSession.projection}
            />
            <Toaster position="top-center" richColors theme={appearance} />
          </ShutdownProvider>
        </ConnectionProvider>
      </InstallationProvider>
    </UpdateProvider>
  );
}

export default App;
