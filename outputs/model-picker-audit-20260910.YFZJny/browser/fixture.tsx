import React, { useEffect, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { ModelPicker } from '@/configuration/ModelPicker';
import type { AccountModel } from '@/account/session';
import catalog from '@/model-profiles/catalog.json';
import '@/index.css';
import '@/workbench/workbench-v2.css';
import './fixture.css';

const models = [...catalog.models.map(m => m.id), ...Array.from({length:60}, (_,i) => `fixture/model-${String(i+1).padStart(2,'0')}`)]
  .map(id => ({id, description:'Fixture private upstream description'})) as AccountModel[];
function Fixture() {
  const [value, setValue] = useState('gpt-6-astra');
  const [priceValue, setPriceValue] = useState('deepseek-v4-flash');
  const [commits, setCommits] = useState(0);
  const [outside, setOutside] = useState(0);
  const [classic, setClassic] = useState(true);
  const [events, setEvents] = useState<string[]>([]);
  useEffect(() => { document.documentElement.dataset.classicScrollbar = String(classic); }, [classic]);
  return <main className="picker-fixture" data-classic-scrollbar={classic}>
    <h1>模型选择框 · 隔离验证</h1>
    <p>合成模型列表，无真实账号、配置写入或应用进程操作。常驻滚动条仅模拟 Windows 交互，不代表 Windows 真机验收。</p>
    <p>选择提交 <output data-testid="commits">{commits}</output> 次；完整 ID：<code data-testid="selected">{value}</code>；外部操作 <output data-testid="outside">{outside}</output> 次</p>
    <button onClick={() => setOutside(x => x + 1)}>外部操作</button>
    <button onClick={() => setClassic(x => !x)}>切换常驻滚动条</button>
    <button onClick={() => document.documentElement.dataset.theme = document.documentElement.dataset.theme === 'dark' ? 'light' : 'dark'}>切换主题</button>
    <section onBlurCapture={e => setEvents(log => [...log.slice(-7), `blur ${e.target.tagName}; related=${e.relatedTarget?.tagName ?? 'null'}; class=${e.relatedTarget?.className ?? ''}`])}>
      <ModelPicker models={models} value={value} label="接入模型" onChange={id => {setValue(id);setCommits(x=>x+1);}} />
    </section>
    <pre data-testid="events">{events.join('\n')}</pre>
    <section className="fixture-price-picker">
      <ModelPicker models={models} value={priceValue} label="价格模型" onChange={setPriceValue} />
    </section>
    <div style={{height:900}}>长页面留白：列表键盘导航不应拖动整个页面。</div>
  </main>;
}
createRoot(document.getElementById('root')!).render(<React.StrictMode><Fixture /></React.StrictMode>);
