/**
 * Material catalog — mirrors crates/export/presets/catalog.json
 * (kept identical by nbcad-export build.rs).
 */
import catalogJson from './catalog.json';
import type { BodyAppearance, Rgba8 } from '../engine/types';

export interface MaterialPresetDto {
  id: string;
  brand: string;
  filament_type: string;
  material_name: string;
  color_name: string;
  r: number;
  g: number;
  b: number;
  filament_id: string | null;
  density_g_cm3: number | null;
  diameter_mm: number;
}

const RAW = catalogJson as MaterialPresetDto[];

export function materialCatalog(): MaterialPresetDto[] {
  return RAW;
}

export function materialBrands(): string[] {
  const brands: string[] = [];
  for (const preset of RAW) {
    if (!brands.includes(preset.brand)) brands.push(preset.brand);
  }
  return brands;
}

export function presetsForBrand(brand: string): MaterialPresetDto[] {
  return RAW.filter((preset) => preset.brand.toLowerCase() === brand.toLowerCase());
}

export function findPreset(id: string): MaterialPresetDto | undefined {
  return RAW.find((preset) => preset.id === id);
}

export function presetToAppearance(preset: MaterialPresetDto, bodyId: number): BodyAppearance {
  const color: Rgba8 = { r: preset.r, g: preset.g, b: preset.b, a: 255 };
  return {
    body_id: bodyId,
    color,
    material_name: preset.material_name,
    filament_type: preset.filament_type,
    brand: preset.brand,
    color_name: preset.color_name,
    filament_id: preset.filament_id,
    preset_id: preset.id,
    density_g_cm3: preset.density_g_cm3,
    diameter_mm: preset.diameter_mm,
  };
}

export type SlicerTargetId =
  | 'standard'
  | 'bambu_studio'
  | 'orca_slicer'
  | 'prusa_slicer'
  | 'cura';

/** Choices in the appearance panel. Bambu and Orca stay in the enum for MCP callers; they write the same file as `standard`. */
export const SLICER_TARGETS: Array<{ id: SlicerTargetId; labelKey: string }> = [
  { id: 'standard', labelKey: 'bodyAppearance.slicerStandard' },
  { id: 'prusa_slicer', labelKey: 'bodyAppearance.slicerPrusa' },
  { id: 'cura', labelKey: 'bodyAppearance.slicerCura' },
];

const SLICER_KEY = 'nbcad:slicerTarget:v1';

export function readSlicerTarget(): SlicerTargetId {
  const value = localStorage.getItem(SLICER_KEY);
  // Older builds stored these as their own menu entries. They write the standard model.
  if (value === 'bambu_studio' || value === 'orca_slicer') return 'standard';
  if (value === 'prusa_slicer' || value === 'cura' || value === 'standard') return value;
  return 'standard';
}

export function writeSlicerTarget(target: SlicerTargetId): void {
  localStorage.setItem(SLICER_KEY, target);
}
