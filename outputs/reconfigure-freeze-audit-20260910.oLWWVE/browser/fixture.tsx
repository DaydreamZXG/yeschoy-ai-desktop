import React, { useState } from 'react';
import { createRoot } from 'react-dom/client';
import { ConfirmDialog } from '@/components/ConfirmDialog';
import '@/i18n';
import '@/index.css';
import '@/workbench/workbench-v2.css';

function Fixture() {
  const [open, setOpen] = useState(false);
  const [pending, setPending] = useState(false);
  const [completed, setCompleted] = useState(1);
  const [probe, setProbe] = useState(0);
  return <>
    <main style={{ height: '100vh', overflowY: 'auto', padding: 32, boxSizing: 'border-box' }}>
      <h1>重新接入 · 隔离复现</h1>
      <p>已接入，模拟 Codex 仍在运行。没有原生接口、真实账号或应用进程操作。</p>
      <p>已完成接入 <output data-testid="completed">{completed}</output> 次；页面点击 <output data-testid="probe">{probe}</output> 次。</p>
      <button onClick={() => setOpen(true)}>换模型并一键接入</button>
      <button onClick={() => setProbe(value => value + 1)}>打开设置（隔离探针）</button>
      <button onClick={() => document.documentElement.dataset.theme = document.documentElement.dataset.theme === 'dark' ? 'light' : 'dark'}>切换主题</button>
      {pending && <p role="status">正在模拟重新接入 <button onClick={() => { setPending(false); setCompleted(value => value + 1); }}>完成模拟请求</button></p>}
      <div style={{ minHeight: 1000 }}>模拟长页面；确认框必须始终位于窗口内。</div>
    </main>
    <ConfirmDialog isOpen={open} title="保存后自动重新打开 Codex Desktop"
      message={'这个应用正在运行。请先保存正在编辑的内容。\n\n继续后，助手会先请求它正常退出，设置完成后再重新打开。本页面仅模拟此流程，不操作任何实际应用。'}
      variant="info" confirmText="已保存，退出并继续" cancelText="暂不接入" pending={pending}
      onConfirm={() => { setOpen(false); setPending(true); }} onCancel={() => setOpen(false)} />
  </>;
}
createRoot(document.getElementById('root')!).render(<React.StrictMode><Fixture /></React.StrictMode>);
