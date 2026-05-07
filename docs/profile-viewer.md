# Cost Explorer

Explore estimated proving costs for WebAssembly programs. Select a program below to view instruction histograms and cost breakdowns.

```{raw} html
<link rel="stylesheet" href="_static/profile-viewer.css">

<div id="profile-viewer">
    <div id="profile-selector" style="margin-bottom: 16px;">
        <label for="profile-select" style="font-weight: 600; margin-right: 8px;">Program:</label>
        <select id="profile-select">
            <option value="sha256">sha256</option>
            <option value="json">json</option>
        </select>
        <span id="profile-description" style="margin-left: 12px; font-size: 0.85rem; color: #757575;"></span>
    </div>

    <div id="content">
        <h1 id="bench-name"></h1>
        <div class="summary-row">
            <div class="summary-panel" id="panel-execution"></div>
            <div class="summary-panel" id="panel-cost"></div>
        </div>

        <div style="flex:1;min-height:200px;display:flex;gap:16px;overflow:hidden;">
            <div class="pv-section" style="flex:1;display:flex;flex-direction:column;overflow:hidden;">
                <div class="hist-toolbar">
                    <div class="control-group">
                        <span class="control-group-label">Weight</span>
                        <div class="control-options">
                            <label><input type="radio" name="hist-weight" value="count" checked /> Count</label>
                            <label><input type="radio" name="hist-weight" value="cost" /> Cost</label>
                        </div>
                    </div>
                    <div class="sep"></div>
                    <div class="control-group" id="group-toggle">
                        <span class="control-group-label">Group</span>
                        <div class="control-options">
                            <input type="checkbox" id="hist-group" />
                        </div>
                    </div>
                    <div class="sep"></div>
                    <div class="control-group disabled" id="unit-group">
                        <span class="control-group-label">Unit</span>
                        <div class="control-options">
                            <label><input type="radio" name="cost-unit" value="muls" checked /> sVOLE</label>
                            <label><input type="radio" name="cost-unit" value="bytes" /> Bytes</label>
                        </div>
                    </div>
                </div>
                <h2 id="hist-title" style="font-size:1rem;color:#424242;margin-bottom:2px;flex-shrink:0;text-align:center;"></h2>
                <div id="hist-subtitle" style="font-size:0.78rem;color:#9e9e9e;margin-bottom:8px;flex-shrink:0;text-align:center;"></div>
                <div id="hist-content" style="flex:1;overflow-y:auto;"></div>
            </div>
            <div class="pv-section" style="flex:0 0 auto;overflow-y:auto;">
                <div style="font-size:0.85rem;font-weight:600;color:#616161;margin-bottom:8px;">Cost Table<span class="help-icon" id="cost-help">?</span></div>
                <div id="cost-editor" style="font-size:0.78rem;"></div>
            </div>
        </div>
    </div>

    <div id="cost-tooltip">Instruction cost depends on control flow visibility. Under <b>public</b> control flow, instructions are proven via optimized circuits (cost in sVOLE). Under <b>private</b> control flow&mdash;when the program branches on a secret value&mdash;execution switches to step-by-step proving, where each CPU step has a fixed cost (sVOLE/step &times; steps).</div>
</div>

<script src="_static/profile-viewer.js"></script>
```
