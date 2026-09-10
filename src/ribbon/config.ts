/** Product command catalog shared by the ribbon, API, and MCP. */
import catalog from '../../interface/catalog.json';
/** Actions the ribbon can dispatch into app state. */
export type RibbonAction =
  | 'enterSketch'
  | 'exitSketch'
  | 'extrude'
  | 'revolve'
  | 'sweep'
  | 'loft'
  | 'rib'
  | 'solidFillet'
  | 'solidChamfer'
  | 'hole'
  | 'constructionPlane'
  | 'constructionVisibility'
  | 'bodyFeature'
  | 'sketchPattern'
  | 'selectTool'
  | 'sketchTool'
  | 'applyConstraint'
  | 'drawingWorkspace'
  | 'assemblyWorkspace'
  | 'modelWorkspace'
  | 'joint'
  | 'drawingNewSheet'
  | 'drawingAutoLayout'
  | 'drawingAddView'
  | 'drawingTool'
  | 'drawingExportDxf'
  | 'drawingExportProfileDxf'
  | 'drawingPrint';

export type MenuEntry =
  | {
      type: 'item';
      id: string;
      labelKey: string;
      icon?: string;
      shortcut?: string;
      enabled?: boolean;
      action?: RibbonAction;
      /** Action argument (tool id for sketchTool, icon id for applyConstraint). */
      payload?: string;
      /** Present means the row owns a hover flyout submenu. */
      children?: MenuEntry[];
    }
  | { type: 'separator' };

export interface RibbonButton {
  id: string;
  labelKey: string;
  icon: string;
  enabled?: boolean;
  action?: RibbonAction;
  payload?: string;
}

export interface RibbonPanel {
  id: string;
  labelKey: string;
  operations: string[];
  buttons: RibbonButton[];
  /** Optional extended command list opened from the panel label. */
  menu?: MenuEntry[];
}

export interface RibbonTab {
  id: string;
  labelKey: string;
  enabled: boolean;
  panels: RibbonPanel[];
}


function workspace(id: string): RibbonTab {
  const tab = catalog.workspaces.find(tab=>tab.id===id);
  if (!tab) throw new Error(`Missing product workspace: ${id}`);
  return tab as RibbonTab;
}
export const SOLID_TAB = workspace('solid');
export const SKETCH_TAB = workspace('sketch');
export const DRAWING_TAB = workspace('drawing');
export const ASSEMBLY_TAB = workspace('assembly');
export const SOLID_WORKSPACE_TABS = [SOLID_TAB].map(({id,labelKey,enabled})=>({id,labelKey,enabled}));
export function ribbonTabById(id: string): RibbonTab { return catalog.workspaces.find(t=>t.id===id) as RibbonTab ?? SOLID_TAB; }
