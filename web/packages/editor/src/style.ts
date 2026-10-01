export const styles = `
.markview-editor{--mv-paper:#f9fafc;--mv-source:#edf2f6;--mv-ink:#273240;--mv-muted:#64758a;--mv-rule:#c4d1dd;--mv-accent:#1d7577;--mv-split:50%;display:flex;width:100%;height:100%;min-height:0;min-width:0;container-type:inline-size;color:var(--mv-ink);background:var(--mv-paper);font:14px/1.5 "Noto Sans","Segoe UI",sans-serif}
.markview-editor[data-theme=dark]{--mv-paper:#171e29;--mv-source:#202b3a;--mv-ink:#e5ebf3;--mv-muted:#a5b4c7;--mv-rule:#3e5064;--mv-accent:#73c6c3}
.markview-editor .mv-toc{width:180px;flex-shrink:0;overflow:auto;border-right:1px solid var(--mv-rule);padding:16px 10px;box-sizing:border-box}
.markview-editor .mv-label{color:var(--mv-muted);font-size:11px;font-weight:600;letter-spacing:.12em;text-transform:uppercase;padding:10px 16px;border-bottom:1px solid var(--mv-rule);flex-shrink:0}
.markview-editor .mv-toc .mv-label{border:0;padding:0 10px 14px}
.markview-editor .mv-toc button{display:block;width:100%;border:0;border-left:2px solid transparent;background:none;color:var(--mv-muted);text-align:left;padding:7px 8px;font:inherit;line-height:1.4;cursor:pointer;overflow-wrap:anywhere}
.markview-editor .mv-toc button:hover{color:var(--mv-ink)}
.markview-editor .mv-toc button[aria-current]{border-color:var(--mv-accent);color:var(--mv-accent);background:var(--mv-source)}
.markview-editor .mv-toc button:focus-visible,.markview-editor .mv-divider:focus-visible{outline:2px solid var(--mv-accent);outline-offset:-2px}
.markview-editor .mv-empty{padding:8px 10px;color:var(--mv-muted);font-size:12px}
.markview-editor .mv-split{display:flex;flex:1;min-width:0;min-height:0;overflow:hidden}
.markview-editor .mv-pane{display:flex;flex-direction:column;min-width:0;min-height:0;overflow:hidden}
.markview-editor .mv-write{flex:0 0 calc(var(--mv-split) - 5px);background:var(--mv-source)}
.markview-editor .mv-read{flex:1;background:var(--mv-paper)}
.markview-editor .mv-content{flex:1;min-height:0;overflow:hidden}
.markview-editor .mv-divider{flex:0 0 10px;cursor:col-resize;touch-action:none;position:relative;background:var(--mv-paper);border:0;border-inline:1px solid var(--mv-rule)}
.markview-editor .mv-divider:after{content:"";position:absolute;top:calc(50% - 15px);left:3px;width:2px;height:30px;border-inline:1px solid var(--mv-muted)}
.markview-editor[data-orientation=vertical] .mv-split{flex-direction:column}
.markview-editor[data-orientation=vertical] .mv-divider{cursor:row-resize;border:0;border-block:1px solid var(--mv-rule)}
.markview-editor[data-orientation=vertical] .mv-divider:after{top:3px;left:calc(50% - 15px);width:30px;height:2px;border:0;border-block:1px solid var(--mv-muted)}
@container(max-width:720px){.markview-editor .mv-toc{display:none}}
@container(max-width:540px){.markview-editor[data-orientation=auto] .mv-split{flex-direction:column}.markview-editor[data-orientation=auto] .mv-divider{cursor:row-resize;border:0;border-block:1px solid var(--mv-rule)}.markview-editor[data-orientation=auto] .mv-divider:after{top:3px;left:calc(50% - 15px);width:30px;height:2px;border:0;border-block:1px solid var(--mv-muted)}}
`;
