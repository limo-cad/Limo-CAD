#include "limo-cad-occt/src/native.rs.h"
#include "refinement_budget.hpp"

#include <APIHeaderSection_MakeHeader.hxx>
#include <BRepAdaptor_Curve.hxx>
#include <BRepAdaptor_Surface.hxx>
#include <BRepAlgoAPI_Common.hxx>
#include <BRepAlgoAPI_Cut.hxx>
#include <BRepAlgoAPI_Fuse.hxx>
#include <BRepAlgoAPI_Section.hxx>
#include <BRepAlgoAPI_Splitter.hxx>
#include <BRepBndLib.hxx>
#include <BRepClass3d.hxx>
#include <BRepClass3d_SolidClassifier.hxx>
#include <BRepBuilderAPI_Copy.hxx>
#include <BRepBuilderAPI_MakeEdge.hxx>
#include <BRepBuilderAPI_MakeFace.hxx>
#include <BRepBuilderAPI_MakePolygon.hxx>
#include <BRepBuilderAPI_MakeWire.hxx>
#include <BRepBuilderAPI_Sewing.hxx>
#include <BRepBuilderAPI_Transform.hxx>
#include <BRepBuilderAPI_TransitionMode.hxx>
#include <BRepCheck_Analyzer.hxx>
#include <BRepClass_FaceClassifier.hxx>
#include <BRepExtrema_DistShapeShape.hxx>
#include <BRepFill.hxx>
#include <BRepMesh_IncrementalMesh.hxx>
#include <BRepMesh_Context.hxx>
#include <BRepMesh_EdgeDiscret.hxx>
#include <BRepMesh_FaceChecker.hxx>
#include <BRepMesh_GeomTool.hxx>
#include <BRepMesh_SphereRangeSplitter.hxx>
#include <BRepMesh_DelabellaMeshAlgoFactory.hxx>
#include <BRepMesh_MeshAlgoFactory.hxx>
#include <IMeshTools_MeshAlgo.hxx>
#include <IMeshData_Model.hxx>
#include <IMeshData_Face.hxx>
#include <IMeshData_Wire.hxx>
#include <IMeshData_Edge.hxx>
#include <ElCLib.hxx>
#include <BRepFilletAPI_MakeChamfer.hxx>
#include <BRepFilletAPI_MakeFillet.hxx>
#include <BRepGProp.hxx>
#include <BRepLib.hxx>
#include <BRepOffsetAPI_MakePipeShell.hxx>
#include <BRepOffsetAPI_MakeThickSolid.hxx>
#include <BRepOffsetAPI_ThruSections.hxx>
#include <BRepPrimAPI_MakeCone.hxx>
#include <BRepPrimAPI_MakeCylinder.hxx>
#include <BRepPrimAPI_MakeHalfSpace.hxx>
#include <BRepPrimAPI_MakePrism.hxx>
#include <BRepPrimAPI_MakeRevol.hxx>
#include <BRepTools.hxx>
#include <BRepTools_WireExplorer.hxx>
#include <BRep_Tool.hxx>
#include <BRep_Builder.hxx>
#include <Bnd_Box.hxx>
#include <GeomAbs_CurveType.hxx>
#include <GeomAbs_Shape.hxx>
#include <GeomAbs_SurfaceType.hxx>
#include <Geom2d_Line.hxx>
#include <Geom2dAdaptor_Curve.hxx>
#include <Geom2d_BSplineCurve.hxx>
#include <Geom2dAPI_InterCurveCurve.hxx>
#include <Geom2d_TrimmedCurve.hxx>
#include <Geom_CylindricalSurface.hxx>
#include <Geom_BSplineCurve.hxx>
#include <Geom_BSplineSurface.hxx>
#include <Geom_TrimmedCurve.hxx>
#include <GeomConvert.hxx>
#include <GC_MakeSegment.hxx>
#include <TColgp_Array2OfPnt.hxx>
#include <TColStd_Array2OfReal.hxx>
#include <TColStd_Array1OfInteger.hxx>
#include <GCPnts_UniformDeflection.hxx>
#include <CPnts_UniformDeflection.hxx>
#include <Precision.hxx>
#include <GC_MakeArcOfCircle.hxx>
#include <GProp_GProps.hxx>
#include <HLRAlgo_Projector.hxx>
#include <HLRBRep_Algo.hxx>
#include <HLRBRep_HLRToShape.hxx>
#include <Message_ProgressRange.hxx>
#include <Message_ProgressIndicator.hxx>
#include <Message_ProgressScope.hxx>
#include <Message.hxx>
#include <Message_Messenger.hxx>
#include <Message_PrinterOStream.hxx>
#include <IFSelect_ReturnStatus.hxx>
#include <Interface_Static.hxx>
#include <Interface_HArray1OfHAsciiString.hxx>
#include <Poly_Triangulation.hxx>
#include <Poly_PolygonOnTriangulation.hxx>
#include <OSD_Environment.hxx>
#include <STEPControl_StepModelType.hxx>
#include <STEPControl_Reader.hxx>
#include <Standard_OutOfMemory.hxx>
#include <ShapeUpgrade_UnifySameDomain.hxx>
#include <STEPControl_Writer.hxx>
#include <ShapeFix_Shape.hxx>
#include <ShapeFix_Solid.hxx>
#include <StepData_StepModel.hxx>
#include <TCollection_HAsciiString.hxx>
#include <TopAbs_Orientation.hxx>
#include <TopAbs_ShapeEnum.hxx>
#include <TopExp.hxx>
#include <TopExp_Explorer.hxx>
#include <TopLoc_Location.hxx>
#include <TopTools_IndexedMapOfShape.hxx>
#include <TopTools_IndexedDataMapOfShapeListOfShape.hxx>
#include <TopTools_ListIteratorOfListOfShape.hxx>
#include <TopTools_ListOfShape.hxx>
#include <TopoDS.hxx>
#include <TopoDS_Edge.hxx>
#include <TopoDS_Face.hxx>
#include <TopoDS_Compound.hxx>
#include <TopoDS_Shape.hxx>
#include <TopoDS_Shell.hxx>
#include <TopoDS_Solid.hxx>
#include <TopoDS_Vertex.hxx>
#include <TopoDS_Wire.hxx>
#include <gp_Ax3.hxx>
#include <gp_Ax1.hxx>
#include <gp_Ax2.hxx>
#include <gp_Circ.hxx>
#include <gp_Cone.hxx>
#include <gp_Cylinder.hxx>
#include <gp_Dir.hxx>
#include <gp_Dir2d.hxx>
#include <gp_Pln.hxx>
#include <gp_Pnt.hxx>
#include <gp_Pnt2d.hxx>
#include <gp_Quaternion.hxx>
#include <gp_Sphere.hxx>
#include <gp_Torus.hxx>
#include <gp_Trsf.hxx>
#include <gp_Vec.hxx>

#include <algorithm>
#include <chrono>
#include <array>
#include <cmath>
#include <optional>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <map>
#include <limits>
#include <set>
#include <sstream>
#include <stdexcept>
#include <string>
#include <utility>
#include <vector>

namespace limo_cad_occt {
namespace {

constexpr double kPi = 3.14159265358979323846;
constexpr double kTau = kPi * 2.0;

class SectionProgress final : public Message_ProgressIndicator {
 public:
  explicit SectionProgress(std::uint64_t timeout_ms)
      : deadline_(std::chrono::steady_clock::now() +
                  std::chrono::milliseconds(timeout_ms)) {}
  void check(const char* stage) const {
    if (expired()) {
      throw std::runtime_error(std::string("Section inspection timed out during ") + stage);
    }
  }
 protected:
  Standard_Boolean UserBreak() override { return expired(); }
  void Show(const Message_ProgressScope&, Standard_Boolean) override {}
 private:
  bool expired() const { return std::chrono::steady_clock::now() >= deadline_; }
  const std::chrono::steady_clock::time_point deadline_;
};

struct SectionMeshBudget {
  std::size_t vertices;
  std::size_t edge_points;
  const SectionProgress* progress;
};

double bounded_through_depth(const TopoDS_Shape& shape, double margin) {
  Bnd_Box bounds;
  BRepBndLib::Add(shape, bounds);
  if (bounds.IsVoid()) {
    throw std::runtime_error("could not bound the through-hole target");
  }
  double x_min = 0.0;
  double y_min = 0.0;
  double z_min = 0.0;
  double x_max = 0.0;
  double y_max = 0.0;
  double z_max = 0.0;
  bounds.Get(x_min, y_min, z_min, x_max, y_max, z_max);
  const double diagonal =
      std::hypot(std::hypot(x_max - x_min, y_max - y_min), z_max - z_min);
  if (!std::isfinite(diagonal) || diagonal <= 0.0) {
    throw std::runtime_error("through-hole target bounds are degenerate");
  }
  return diagonal + std::max(margin, 1.0);
}

double bounded_directional_depth(
    const TopoDS_Shape& shape,
    const gp_Pnt& origin,
    const gp_Vec& unit_direction) {
  Bnd_Box bounds;
  BRepBndLib::Add(shape, bounds);
  if (bounds.IsVoid()) {
    throw std::runtime_error("could not bound the threaded-hole target");
  }
  double x_min = 0.0;
  double y_min = 0.0;
  double z_min = 0.0;
  double x_max = 0.0;
  double y_max = 0.0;
  double z_max = 0.0;
  bounds.Get(x_min, y_min, z_min, x_max, y_max, z_max);
  double depth = 0.0;
  for (const double x : {x_min, x_max}) {
    for (const double y : {y_min, y_max}) {
      for (const double z : {z_min, z_max}) {
        depth = std::max(
            depth, gp_Vec(origin, gp_Pnt(x, y, z)).Dot(unit_direction));
      }
    }
  }
  if (!std::isfinite(depth) || depth <= 0.0) {
    throw std::runtime_error("threaded-hole target depth is degenerate");
  }
  return depth;
}

gp_Pnt helical_point(
    const gp_Ax2& axis,
    double radius,
    double center,
    double angle) {
  const gp_Vec radial =
      gp_Vec(axis.XDirection())
          .Multiplied(std::cos(angle))
          .Added(gp_Vec(axis.YDirection()).Multiplied(std::sin(angle)));
  return axis.Location().Translated(
      radial.Multiplied(radius).Added(
          gp_Vec(axis.Direction()).Multiplied(center)));
}

TopoDS_Edge make_helical_edge(
    double center_start,
    double center_end,
    double pitch,
    bool left_hand,
    const Handle(Geom_CylindricalSurface)& surface,
    double curve_tolerance = 1e-5) {
  const double angle_span = kTau * (center_end - center_start) / pitch;
  const double axial_per_radian = pitch / kTau;
  const double handedness = left_hand ? -1.0 : 1.0;
  const double parameter_span =
      angle_span * std::hypot(1.0, axial_per_radian);
  Handle(Geom2d_Line) pcurve = new Geom2d_Line(
      gp_Pnt2d(0.0, center_start),
      gp_Dir2d(handedness, axial_per_radian));
  BRepBuilderAPI_MakeEdge builder(pcurve, surface, 0.0, parameter_span);
  if (!builder.IsDone()) {
    throw std::runtime_error("OCCT could not build the exact helical edge");
  }
  TopoDS_Edge edge = builder.Edge();
  BRepLib::BuildCurve3d(edge, curve_tolerance);
  BRepLib::SameParameter(edge, 1e-7);
  return edge;
}




TopoDS_Face make_curved_helical_face(
    const gp_Ax2& axis,
    const Handle(Geom_BSplineCurve)& helix,
    const Handle(Geom_Curve)& section) {
  const Handle(Geom_BSplineCurve) profile =
      GeomConvert::CurveToBSplineCurve(section);
  TColgp_Array2OfPnt poles(1, helix->NbPoles(), 1, profile->NbPoles());
  TColStd_Array2OfReal weights(1, helix->NbPoles(), 1, profile->NbPoles());
  for (int u = 1; u <= helix->NbPoles(); ++u) {
    const gp_Pnt h = helix->Pole(u);
    for (int v = 1; v <= profile->NbPoles(); ++v) {
      const gp_Pnt p = profile->Pole(v);
      poles.SetValue(u, v, axis.Location().Translated(
          gp_Vec(axis.XDirection()).Multiplied(h.X() * p.X())
              .Added(gp_Vec(axis.YDirection()).Multiplied(h.Y() * p.X()))
              .Added(gp_Vec(axis.Direction()).Multiplied(h.Z() + p.Z()))));
      weights.SetValue(u, v, helix->Weight(u) * profile->Weight(v));
    }
  }
  TColStd_Array1OfReal u_knots(1, helix->NbKnots());
  TColStd_Array1OfReal v_knots(1, profile->NbKnots());
  TColStd_Array1OfInteger u_mults(1, helix->NbKnots());
  TColStd_Array1OfInteger v_mults(1, profile->NbKnots());
  helix->Knots(u_knots); helix->Multiplicities(u_mults);
  profile->Knots(v_knots); profile->Multiplicities(v_mults);
  Handle(Geom_BSplineSurface) surface = new Geom_BSplineSurface(
      poles, weights, u_knots, v_knots, u_mults, v_mults,
      helix->Degree(), profile->Degree());
  BRepBuilderAPI_MakeFace face(surface, 1e-7);
  if (!face.IsDone()) throw std::runtime_error("could not build rounded helical face");
  return face.Face();
}

TopoDS_Shape make_continuous_thread_cutter(
    const gp_Ax2& axis,
    double spine_radius,
    const std::vector<std::pair<double, double>>& radius_half_widths,
    double pitch,
    double thread_depth,
    bool left_hand,
    const char* label,
    const std::vector<Handle(Geom_Curve)>& curved_profile = {}) {



  const double center_start = -pitch;
  const double center_end = thread_depth + pitch;
  const double turns = (center_end - center_start) / pitch;
  if (!std::isfinite(turns) || turns <= 0.0 || turns > 256.0) {
    throw std::runtime_error(
        std::string(label) +
        " thread interval is too short or exceeds 256 turns; use simplified representation");
  }

  const double inner_radius = radius_half_widths.front().first;
  const double outer_radius = radius_half_widths.back().first;
  if (!std::isfinite(spine_radius) || spine_radius <= inner_radius ||
      spine_radius >= outer_radius) {
    throw std::runtime_error("thread spine must lie inside its radial profile");
  }
  double previous_radius = -1.0;
  for (const auto& station : radius_half_widths) {
    if (!std::isfinite(station.first) || !std::isfinite(station.second) ||
        station.first <= previous_radius || station.second <= 0.0 ||
        station.second >= pitch * 0.5) {
      throw std::runtime_error("thread profile radial stations are invalid");
    }
    previous_radius = station.first;
  }






  BRepBuilderAPI_Sewing sewing(1e-7, true, true, true, false);
  const int segment_count = static_cast<int>(std::ceil(turns - 1e-10));
  for (int segment_index = 0; segment_index < segment_count;
       ++segment_index) {
    const double segment_start = center_start + segment_index * pitch;
    const double segment_end = std::min(segment_start + pitch, center_end);
    if (!curved_profile.empty()) {
      Handle(Geom_CylindricalSurface) unit_surface = new Geom_CylindricalSurface(
          gp_Ax3(gp_Pnt(0, 0, 0), gp_Dir(0, 0, 1)), 1.0);
      const TopoDS_Edge spine = make_helical_edge(
          segment_start, segment_end, pitch, left_hand, unit_surface, 1e-10);
      double first, last;
      Handle(Geom_Curve) curve = BRep_Tool::Curve(spine, first, last);
      Handle(Geom_BSplineCurve) helix = GeomConvert::CurveToBSplineCurve(
          new Geom_TrimmedCurve(curve, first, last));
      for (const auto& section : curved_profile) {
        sewing.Add(make_curved_helical_face(axis, helix, section));
      }
      continue;
    }
    std::vector<TopoDS_Edge> lower_rails;
    std::vector<TopoDS_Edge> upper_rails;
    lower_rails.reserve(radius_half_widths.size());
    upper_rails.reserve(radius_half_widths.size());
    for (const auto& station : radius_half_widths) {
      Handle(Geom_CylindricalSurface) rail_surface =
          new Geom_CylindricalSurface(gp_Ax3(axis), station.first);
      lower_rails.push_back(make_helical_edge(
          segment_start - station.second, segment_end - station.second,
          pitch, left_hand, rail_surface));
      upper_rails.push_back(make_helical_edge(
          segment_start + station.second, segment_end + station.second,
          pitch, left_hand, rail_surface));
    }
    sewing.Add(BRepFill::Face(lower_rails.front(), upper_rails.front()));
    sewing.Add(BRepFill::Face(lower_rails.back(), upper_rails.back()));
    for (std::size_t index = 0; index + 1 < lower_rails.size(); ++index) {
      sewing.Add(BRepFill::Face(lower_rails[index], lower_rails[index + 1]));
      sewing.Add(BRepFill::Face(upper_rails[index], upper_rails[index + 1]));
    }
  }

  const double handedness = left_hand ? -1.0 : 1.0;
  const double end_angle = handedness * kTau * turns;
  const auto make_cap = [&](double center, double angle) {
    if (!curved_profile.empty()) {
      const gp_Vec x = gp_Vec(axis.XDirection()).Multiplied(std::cos(angle))
          .Added(gp_Vec(axis.YDirection()).Multiplied(std::sin(angle)));
      const gp_Vec y = gp_Vec(axis.XDirection()).Multiplied(-std::sin(angle))
          .Added(gp_Vec(axis.YDirection()).Multiplied(std::cos(angle)));
      const gp_Vec z(axis.Direction());
      const gp_Pnt origin = axis.Location().Translated(z.Multiplied(center));
      gp_Trsf placement;
      placement.SetValues(x.X(), y.X(), z.X(), origin.X(),
                          x.Y(), y.Y(), z.Y(), origin.Y(),
                          x.Z(), y.Z(), z.Z(), origin.Z());
      BRepBuilderAPI_MakeWire wire;
      for (const auto& section : curved_profile) {
        Handle(Geom_Curve) placed = Handle(Geom_Curve)::DownCast(section->Transformed(placement));
        wire.Add(BRepBuilderAPI_MakeEdge(placed).Edge());
      }
      BRepBuilderAPI_MakeFace face(wire.Wire(), true);
      if (!face.IsDone()) throw std::runtime_error("could not cap rounded thread");
      return face.Face();
    }
    BRepBuilderAPI_MakePolygon polygon;
    for (const auto& station : radius_half_widths) {
      polygon.Add(helical_point(
          axis, station.first, center - station.second, angle));
    }
    for (auto station = radius_half_widths.rbegin();
         station != radius_half_widths.rend(); ++station) {
      polygon.Add(helical_point(
          axis, station->first, center + station->second, angle));
    }
    polygon.Close();
    if (!polygon.IsDone()) {
      throw std::runtime_error(
          std::string("OCCT could not close the continuous ") + label +
          " thread cutter end");
    }
    BRepBuilderAPI_MakeFace face(polygon.Wire(), true);
    if (!face.IsDone()) {
      throw std::runtime_error(
          std::string("OCCT could not cap the continuous ") + label +
          " thread cutter");
    }
    return face.Face();
  };
  sewing.Add(make_cap(center_start, 0.0));
  sewing.Add(make_cap(center_end, end_angle));
  sewing.Perform(Message_ProgressRange());
  if (sewing.SewedShape().IsNull() || sewing.NbFreeEdges() != 0 ||
      sewing.NbMultipleEdges() != 0) {
    throw std::runtime_error(
        std::string("OCCT could not sew the continuous ") + label +
        " thread cutter into a closed shell (free edges=" +
        std::to_string(sewing.NbFreeEdges()) + ", multiple edges=" +
        std::to_string(sewing.NbMultipleEdges()) + ")");
  }
  TopExp_Explorer shells(sewing.SewedShape(), TopAbs_SHELL);
  if (!shells.More()) {
    throw std::runtime_error(
        std::string("OCCT continuous ") + label +
        " thread boundary did not produce a shell");
  }
  const TopoDS_Shell shell = TopoDS::Shell(shells.Current());
  shells.Next();
  if (shells.More()) {
    throw std::runtime_error(
        std::string("OCCT continuous ") + label +
        " thread boundary produced multiple shells");
  }
  ShapeFix_Solid solid_fixer;
  solid_fixer.SetPrecision(1e-7);
  TopoDS_Solid cutter = solid_fixer.SolidFromShell(shell);
  if (cutter.IsNull()) {
    throw std::runtime_error(
        std::string("OCCT could not solidify the continuous ") + label +
        " thread boundary");
  }
  if (!BRepLib::OrientClosedSolid(cutter)) {
    throw std::runtime_error(
        std::string("OCCT could not orient the continuous ") + label +
        " thread cutter solid");
  }
  BRepLib::SameParameter(cutter, 1e-6, true);

  const double sample_center = (center_start + center_end) * 0.5;
  const double sample_angle =
      (left_hand ? -1.0 : 1.0) * kTau *
      (sample_center - center_start) / pitch;
  const gp_Pnt sample_point = helical_point(
      axis, (inner_radius + outer_radius) * 0.5, sample_center,
      sample_angle);
  const gp_Pnt inner_probe = helical_point(
      axis, inner_radius + (outer_radius - inner_radius) * 0.1,
      sample_center, sample_angle);
  const gp_Pnt outer_probe = helical_point(
      axis, outer_radius - (outer_radius - inner_radius) * 0.1,
      sample_center, sample_angle);
  const double boundary_probe =
      std::max(1e-5, (outer_radius - inner_radius) * 1e-3);
  const gp_Pnt inside_inner_boundary = helical_point(
      axis, inner_radius + boundary_probe, sample_center, sample_angle);
  const gp_Pnt outside_inner_boundary = helical_point(
      axis, inner_radius - boundary_probe, sample_center, sample_angle);
  const gp_Pnt inside_outer_boundary = helical_point(
      axis, outer_radius - boundary_probe, sample_center, sample_angle);
  const gp_Pnt outside_outer_boundary = helical_point(
      axis, outer_radius + boundary_probe, sample_center, sample_angle);
  const gp_Pnt axis_probe = axis.Location().Translated(
      gp_Vec(axis.Direction()).Multiplied(sample_center));
  const auto is_inside = [&](const gp_Pnt& point) {
    BRepClass3d_SolidClassifier classifier(cutter, point, 1e-7);
    return classifier.State() == TopAbs_IN ||
           classifier.State() == TopAbs_ON;
  };



  if (!is_inside(sample_point) || !is_inside(inner_probe) ||
      !is_inside(outer_probe) || !is_inside(inside_inner_boundary) ||
      is_inside(outside_inner_boundary) ||
      !is_inside(inside_outer_boundary) ||
      is_inside(outside_outer_boundary) || is_inside(axis_probe)) {
    Bnd_Box bounds;
    BRepBndLib::Add(cutter, bounds);
    std::ostringstream details;
    if (!bounds.IsVoid()) {
      double x_min = 0.0;
      double y_min = 0.0;
      double z_min = 0.0;
      double x_max = 0.0;
      double y_max = 0.0;
      double z_max = 0.0;
      bounds.Get(x_min, y_min, z_min, x_max, y_max, z_max);
      details << " bounds=[" << x_min << "," << y_min << "," << z_min
              << "]-[" << x_max << "," << y_max << "," << z_max << "]";
    }
    details << " probes=" << is_inside(sample_point) << ","
            << is_inside(inner_probe) << "," << is_inside(outer_probe)
            << ",inner=" << is_inside(outside_inner_boundary) << "/"
            << is_inside(inside_inner_boundary) << ",outer="
            << is_inside(inside_outer_boundary) << "/"
            << is_inside(outside_outer_boundary)
            << ",axis=" << is_inside(axis_probe);
    throw std::runtime_error(
        std::string("OCCT continuous ") + label +
        " thread cutter is inside-out" + details.str());
  }
  BRepCheck_Analyzer analyzer(cutter, true, false);
  if (!analyzer.IsValid()) {
    throw std::runtime_error(
        std::string("OCCT continuous ") + label +
        " thread cutter is invalid");
  }
  GProp_GProps properties;
  BRepGProp::VolumeProperties(cutter, properties);
  if (!std::isfinite(properties.Mass()) ||
      std::abs(properties.Mass()) <= 1e-9) {
    throw std::runtime_error(
        std::string("OCCT continuous ") + label +
        " thread cutter has no volume");
  }
  return cutter;
}

std::vector<TopoDS_Shape> make_rounded_thread_cutters(
    const gp_Ax2& axis, double major, double minor, double pitch,
    double radius, double axial_clearance, double depth, bool left_hand,
    bool internal) {
  const double lo = minor * 0.5;
  const double hi = major * 0.5;
  const double beta = kPi / 12.0;
  const double h0 = pitch * 0.25 - (hi - lo) * 0.5 * std::tan(beta);
  const double h1 = pitch * 0.25 + (hi - lo) * 0.5 * std::tan(beta);
  const double corner = radius * (1.0 / std::cos(beta) - std::tan(beta));
  const double z0 = h0 - corner;
  const double z1 = h1 + corner;
  const double overlap = std::max(0.005, pitch * 0.02);
  if (lo <= overlap || radius <= 0 ||
      2 * radius * (1 - std::sin(beta)) >= hi - lo ||
      z0 <= axial_clearance * 0.5 || z1 + axial_clearance * 0.5 >= pitch * 0.5) {
    throw std::runtime_error("rounded trapezoidal thread profile is invalid");
  }


  const auto point = [&](double r, double z) {
    return gp_Pnt(r, 0, internal ? pitch * 0.5 - z + axial_clearance * 0.5 : z);
  };
  const auto arc_point = [&](double cr, double cz, double angle) {
    return point(cr + radius * std::cos(angle), cz + radius * std::sin(angle));
  };
  const gp_Pnt root = point(lo, z0);
  const gp_Pnt root_tangent = arc_point(lo + radius, z0, kPi * 0.5 + beta);
  const gp_Pnt crest_tangent = arc_point(hi - radius, z1, beta - kPi * 0.5);
  const gp_Pnt crest = point(hi, z1);
  std::vector<Handle(Geom_Curve)> upper;
  if (internal) upper.push_back(GC_MakeSegment(point(lo - overlap, z0), root).Value());
  upper.push_back(GC_MakeArcOfCircle(root,
      arc_point(lo + radius, z0, (kPi + kPi * 0.5 + beta) * 0.5), root_tangent).Value());
  upper.push_back(GC_MakeSegment(root_tangent, crest_tangent).Value());
  upper.push_back(GC_MakeArcOfCircle(crest_tangent,
      arc_point(hi - radius, z1, (beta - kPi * 0.5) * 0.5), crest).Value());
  if (!internal) upper.push_back(GC_MakeSegment(crest, point(hi + overlap, z1)).Value());
  std::vector<Handle(Geom_Curve)> curves = upper;
  const auto mirror = [](const gp_Pnt& p) { return gp_Pnt(p.X(), 0, -p.Z()); };
  const gp_Pnt start = upper.front()->Value(upper.front()->FirstParameter());
  const gp_Pnt end = upper.back()->Value(upper.back()->LastParameter());
  curves.push_back(GC_MakeSegment(end, mirror(end)).Value());
  gp_Trsf reflection;
  reflection.SetMirror(gp_Ax2(gp_Pnt(0, 0, 0), gp_Dir(0, 0, 1)));
  for (auto it = upper.rbegin(); it != upper.rend(); ++it) {
    Handle(Geom_Curve) lower = Handle(Geom_Curve)::DownCast((*it)->Transformed(reflection));
    lower->Reverse();
    curves.push_back(lower);
  }
  curves.push_back(GC_MakeSegment(mirror(start), start).Value());
  const std::vector<std::pair<double, double>> stations = {
      {start.X(), start.Z()}, {end.X(), end.Z()}};
  TopoDS_Shape cutter = make_continuous_thread_cutter(
      axis, (hi + lo) * 0.5, stations, pitch, depth, left_hand,
      "custom rounded trapezoidal", curves);
  return {cutter};
}

void trim_thread_tools_at_depth(
    std::vector<TopoDS_Shape>& cutters, const gp_Ax2& axis,
    double major_radius, double pitch, double depth, bool bound_start = false) {



  const double start_offset = bound_start ? 0.0 : -pitch;
  const gp_Ax2 clip_axis(
      axis.Location().Translated(gp_Vec(axis.Direction()).Multiplied(start_offset)),
      axis.Direction(), axis.XDirection());
  BRepPrimAPI_MakeCylinder clip(clip_axis, major_radius + pitch, depth - start_offset);
  for (TopoDS_Shape& cutter : cutters) {
    BRepAlgoAPI_Common trimmed(cutter, clip.Shape(), Message_ProgressRange());
    if (!trimmed.IsDone() || trimmed.HasErrors() || trimmed.Shape().IsNull()) {
      throw std::runtime_error("could not trim thread to its requested depth");
    }
    cutter = trimmed.Shape();
  }
}

std::vector<TopoDS_Shape> make_internal_thread_cutters(
    const gp_Ax2& axis,
    double major_diameter,
    double pitch_diameter,
    double minor_diameter,
    double pitch,
    double thread_depth,
    bool left_hand) {
  const double overlap = std::max(
      2e-3, std::min({minor_diameter * 5e-3, pitch * 2e-2,
                      (pitch_diameter - minor_diameter) * 2.5e-2}));
  const double minor_radius = minor_diameter * 0.5;
  const double inner_radius = minor_radius - overlap;
  const double pitch_radius = pitch_diameter * 0.5;
  const double outer_radius = major_diameter * 0.5;
  const double pitch_half_width = pitch * 0.25;
  const double outer_half_width =
      pitch_half_width -
      (outer_radius - pitch_radius) * std::tan(kPi / 6.0);
  const double inner_half_width =
      pitch_half_width +
      (pitch_radius - minor_radius) * std::tan(kPi / 6.0);
  if (inner_radius <= 0.0 || pitch_radius <= inner_radius ||
      outer_radius <= pitch_radius || outer_half_width <= 0.0 ||
      inner_half_width >= pitch * 0.499) {
    throw std::runtime_error(
        "ISO internal thread limits do not form a valid 60-degree profile");
  }
  const std::vector<std::pair<double, double>> profile = {
      {inner_radius, inner_half_width},
      {minor_radius, inner_half_width},
      {outer_radius, outer_half_width},
  };
  return {make_continuous_thread_cutter(
      axis, pitch_radius, profile, pitch, thread_depth, left_hand,
      "internal")};
}

std::vector<TopoDS_Shape> make_external_thread_cutters(
    const gp_Ax2& axis,
    double major_diameter,
    double pitch_diameter,
    double minor_diameter,
    double pitch,
    double thread_depth,
    bool left_hand) {
  const double overlap = std::max(
      5e-3, std::min({major_diameter * 3e-2, pitch * 1.5e-1,
                      (major_diameter - pitch_diameter) * 5e-1}));
  const double inner_radius = minor_diameter * 0.5;
  const double pitch_radius = pitch_diameter * 0.5;
  const double major_radius = major_diameter * 0.5;
  const double outer_radius = major_radius + overlap;
  const double pitch_half_width = pitch * 0.25;
  const double inner_half_width =
      pitch_half_width -
      (pitch_radius - inner_radius) * std::tan(kPi / 6.0);
  const double outer_half_width =
      pitch_half_width +
      (major_radius - pitch_radius) * std::tan(kPi / 6.0);
  if (inner_radius <= 0.0 || pitch_radius <= inner_radius ||
      outer_radius <= pitch_radius || inner_half_width <= 0.0 ||
      outer_half_width >= pitch * 0.499) {
    throw std::runtime_error(
        "ISO external thread limits do not form a valid 60-degree profile");
  }
  const std::vector<std::pair<double, double>> profile = {
      {inner_radius, inner_half_width},
      {major_radius, outer_half_width},
      {outer_radius, outer_half_width},
  };
  return {make_continuous_thread_cutter(
      axis, pitch_radius, profile, pitch, thread_depth, left_hand,
      "external")};
}

TopoDS_Shape cut_thread_tools(
    const TopoDS_Shape& target,
    const std::vector<TopoDS_Shape>& cutters) {
  if (cutters.empty()) {
    return target;
  }
  TopTools_ListOfShape arguments;
  arguments.Append(target);
  TopTools_ListOfShape tools;
  for (const TopoDS_Shape& cutter : cutters) {
    tools.Append(cutter);
  }
  BRepAlgoAPI_Cut cut;
  cut.SetArguments(arguments);
  cut.SetTools(tools);
  cut.SetNonDestructive(true);
  cut.SetRunParallel(true);
  cut.Build(Message_ProgressRange());
  if (!cut.IsDone() || cut.HasErrors() || cut.Shape().IsNull()) {
    throw std::runtime_error("OCCT modeled thread cut failed");
  }
  return cut.Shape();
}

gp_Pnt point_at(const FfiJob& job, std::size_t point_index) {
  const std::size_t offset = point_index * 3;
  if (offset + 2 >= job.points.size()) {
    throw std::runtime_error("profile point buffer is malformed");
  }
  return gp_Pnt(job.points[offset], job.points[offset + 1], job.points[offset + 2]);
}

TopoDS_Wire make_wire(const std::vector<gp_Pnt>& points) {
  if (points.size() < 3) {
    throw std::runtime_error("profile must contain at least three points");
  }
  BRepBuilderAPI_MakePolygon polygon;
  for (const gp_Pnt& point : points) {
    polygon.Add(point);
  }
  polygon.Close();
  if (!polygon.IsDone()) {
    throw std::runtime_error("OCCT could not build the profile wire");
  }
  return polygon.Wire();
}

TopoDS_Wire make_open_wire(const std::vector<gp_Pnt>& points) {
  if (points.size() < 2) {
    throw std::runtime_error("path must contain at least two points");
  }
  BRepBuilderAPI_MakePolygon polygon;
  for (const gp_Pnt& point : points) {
    polygon.Add(point);
  }
  if (!polygon.IsDone()) {
    throw std::runtime_error("OCCT could not build the path wire");
  }
  return polygon.Wire();
}

gp_Pnt buffered_curve_point(const rust::Vec<double>& points,
                            std::size_t point_index,
                            const char* label) {
  const std::size_t offset = point_index * 3;
  if (offset + 2 >= points.size()) {
    throw std::runtime_error(std::string(label) + " curve point buffer is malformed");
  }
  return gp_Pnt(points[offset], points[offset + 1], points[offset + 2]);
}

TopoDS_Wire make_curve_wire(const rust::Vec<std::uint8_t>& kinds,
                            const rust::Vec<std::uint32_t>& offsets,
                            const rust::Vec<double>& points,
                            const char* label) {
  if (kinds.empty() || offsets.size() != kinds.size() + 1 ||
      offsets.front() != 0 || offsets.back() * 3 != points.size()) {
    throw std::runtime_error(std::string(label) + " curve buffers are malformed");
  }
  BRepBuilderAPI_MakeWire wire;
  for (std::size_t curve_index = 0; curve_index < kinds.size(); ++curve_index) {
    const std::size_t begin = offsets[curve_index];
    const std::size_t end = offsets[curve_index + 1];
    const std::size_t count = end - begin;
    auto point = [&](std::size_t index) {
      return buffered_curve_point(points, index, label);
    };
    if (kinds[curve_index] == 0) {
      if (count != 2) {
        throw std::runtime_error(std::string(label) + " line needs two points");
      }
      BRepBuilderAPI_MakeEdge edge(point(begin), point(begin + 1));
      if (!edge.IsDone()) {
        throw std::runtime_error(std::string("OCCT could not build the ") + label +
                                 " line");
      }
      wire.Add(edge.Edge());
    } else if (kinds[curve_index] == 1) {
      if (count != 3) {
        throw std::runtime_error(std::string(label) +
                                 " arc needs start/mid/end points");
      }
      GC_MakeArcOfCircle arc(point(begin), point(begin + 1), point(begin + 2));
      if (!arc.IsDone()) {
        throw std::runtime_error(std::string("OCCT could not build the ") + label +
                                 " arc");
      }
      BRepBuilderAPI_MakeEdge edge(arc.Value());
      if (!edge.IsDone()) {
        throw std::runtime_error(std::string("OCCT could not build the ") + label +
                                 " arc edge");
      }
      wire.Add(edge.Edge());
    } else if (kinds[curve_index] == 2) {
      if (count != 3) {
        throw std::runtime_error(std::string(label) +
                                 " circle needs center/axis/normal data");
      }
      const gp_Pnt center = point(begin);
      const gp_Pnt axis_point = point(begin + 1);
      const gp_Pnt normal_data = point(begin + 2);
      const gp_Vec axis(center, axis_point);
      const gp_Vec normal(normal_data.X(), normal_data.Y(), normal_data.Z());
      if (axis.SquareMagnitude() < 1e-18 || normal.SquareMagnitude() < 1e-18) {
        throw std::runtime_error(std::string(label) + " circle axes are degenerate");
      }
      BRepBuilderAPI_MakeEdge edge(
          gp_Circ(gp_Ax2(center, gp_Dir(normal), gp_Dir(axis)), axis.Magnitude()));
      if (!edge.IsDone()) {
        throw std::runtime_error(std::string("OCCT could not build the ") + label +
                                 " circle");
      }
      wire.Add(edge.Edge());
    } else if (kinds[curve_index] == 3) {
      if (count < 2) {
        throw std::runtime_error(std::string(label) +
                                 " polyline needs at least two points");
      }
      for (std::size_t index = begin; index + 1 < end; ++index) {
        BRepBuilderAPI_MakeEdge edge(point(index), point(index + 1));
        if (!edge.IsDone()) {
          throw std::runtime_error(std::string("OCCT could not build the ") +
                                   label + " polyline");
        }
        wire.Add(edge.Edge());
      }
    } else {
      throw std::runtime_error(std::string("unknown ") + label + " curve kind");
    }
  }
  if (!wire.IsDone()) {
    throw std::runtime_error(std::string("OCCT could not build the ") + label +
                             " wire");
  }
  return wire.Wire();
}

struct SectionTransform {
  gp_Pnt centroid;
  gp_Vec translation;
  double scale;

  gp_Pnt Apply(const gp_Pnt& point) const {
    gp_Vec radial(centroid, point);
    radial.Multiply(scale);
    gp_Pnt transformed = centroid.Translated(radial);
    transformed.Translate(translation);
    return transformed;
  }
};

SectionTransform section_transform(const FfiJob& job, std::size_t begin,
                                   std::size_t end, double offset,
                                   double reference_radius) {
  const gp_Vec normal(job.normal_x, job.normal_y, job.normal_z);
  if (normal.SquareMagnitude() < 1e-18) {
    throw std::runtime_error("extrude normal is degenerate");
  }
  gp_Vec unit = normal.Normalized();
  gp_Pnt centroid(0.0, 0.0, 0.0);
  for (std::size_t index = begin; index < end; ++index) {
    const gp_Pnt point = point_at(job, index);
    centroid.SetX(centroid.X() + point.X());
    centroid.SetY(centroid.Y() + point.Y());
    centroid.SetZ(centroid.Z() + point.Z());
  }
  const double count = static_cast<double>(end - begin);
  centroid.SetX(centroid.X() / count);
  centroid.SetY(centroid.Y() / count);
  centroid.SetZ(centroid.Z() / count);

  const double angle = job.taper_angle_deg * kPi / 180.0;
  const double scale = 1.0 + std::tan(angle) * offset / reference_radius;
  if (!std::isfinite(scale) || scale <= 1e-6) {
    throw std::runtime_error("taper collapses or inverts the profile");
  }
  return SectionTransform{centroid, unit.Multiplied(offset), scale};
}

gp_Pnt curve_point_at(const FfiJob& job, std::size_t point_index) {
  const std::size_t offset = point_index * 3;
  if (offset + 2 >= job.curve_points.size()) {
    throw std::runtime_error("profile curve point buffer is malformed");
  }
  return gp_Pnt(job.curve_points[offset], job.curve_points[offset + 1],
                job.curve_points[offset + 2]);
}

TopoDS_Wire make_profile_wire(const FfiJob& job, std::size_t profile_index,
                              const SectionTransform* transform = nullptr) {
  if (profile_index + 1 >= job.profile_offsets.size()) {
    throw std::runtime_error("profile offset buffer is malformed");
  }
  const std::size_t point_begin = job.profile_offsets[profile_index];
  const std::size_t point_end = job.profile_offsets[profile_index + 1];


  if (job.curve_kinds.empty() || job.curve_profile_offsets.empty()) {
    std::vector<gp_Pnt> points;
    points.reserve(point_end - point_begin);
    for (std::size_t index = point_begin; index < point_end; ++index) {
      const gp_Pnt value = point_at(job, index);
      points.push_back(transform == nullptr ? value : transform->Apply(value));
    }
    return make_wire(points);
  }
  if (job.curve_profile_offsets.size() != job.profile_offsets.size() ||
      job.curve_point_offsets.size() != job.curve_kinds.size() + 1 ||
      job.curve_point_offsets.back() * 3 != job.curve_points.size()) {
    throw std::runtime_error("profile curve buffers are malformed");
  }

  const std::size_t curve_begin = job.curve_profile_offsets[profile_index];
  const std::size_t curve_end = job.curve_profile_offsets[profile_index + 1];
  if (curve_end <= curve_begin || curve_end > job.curve_kinds.size()) {
    throw std::runtime_error("profile contains no boundary curves");
  }

  auto transformed = [&](std::size_t point_index) {
    const gp_Pnt value = curve_point_at(job, point_index);
    return transform == nullptr ? value : transform->Apply(value);
  };
  BRepBuilderAPI_MakeWire wire;
  for (std::size_t curve_index = curve_begin; curve_index < curve_end;
       ++curve_index) {
    const std::size_t begin = job.curve_point_offsets[curve_index];
    const std::size_t end = job.curve_point_offsets[curve_index + 1];
    const std::size_t count = end - begin;
    switch (job.curve_kinds[curve_index]) {
      case 0: {
        if (count != 2) {
          throw std::runtime_error("line curve requires two points");
        }
        BRepBuilderAPI_MakeEdge edge(transformed(begin),
                                     transformed(begin + 1));
        if (!edge.IsDone()) {
          throw std::runtime_error("OCCT could not build a line profile edge");
        }
        wire.Add(edge.Edge());
        break;
      }
      case 1: {
        if (count != 3) {
          throw std::runtime_error("arc curve requires start/mid/end points");
        }
        const gp_Pnt start = transformed(begin);
        const gp_Pnt mid = transformed(begin + 1);
        const gp_Pnt finish = transformed(begin + 2);
        GC_MakeArcOfCircle arc(start, mid, finish);
        if (!arc.IsDone()) {
          throw std::runtime_error("OCCT could not build an analytic arc");
        }
        BRepBuilderAPI_MakeEdge edge(arc.Value());
        if (!edge.IsDone()) {
          throw std::runtime_error("OCCT could not build an arc profile edge");
        }
        wire.Add(edge.Edge());
        break;
      }
      case 2: {
        if (count != 3) {
          throw std::runtime_error(
              "circle curve requires center/axis/normal data");
        }
        const gp_Pnt center = transformed(begin);
        const gp_Pnt axis_point = transformed(begin + 1);
        const gp_Pnt normal_data = curve_point_at(job, begin + 2);
        const gp_Vec axis(center, axis_point);
        const gp_Vec normal(normal_data.X(), normal_data.Y(), normal_data.Z());
        if (axis.SquareMagnitude() < 1e-18 ||
            normal.SquareMagnitude() < 1e-18) {
          throw std::runtime_error("circle curve has degenerate axes");
        }
        const gp_Circ circle(gp_Ax2(center, gp_Dir(normal), gp_Dir(axis)),
                             axis.Magnitude());
        BRepBuilderAPI_MakeEdge edge(circle);
        if (!edge.IsDone()) {
          throw std::runtime_error("OCCT could not build a circle profile edge");
        }
        wire.Add(edge.Edge());
        break;
      }
      case 3: {
        if (count < 2) {
          throw std::runtime_error("polyline curve needs at least two points");
        }
        for (std::size_t index = begin; index + 1 < end; ++index) {
          BRepBuilderAPI_MakeEdge edge(transformed(index),
                                       transformed(index + 1));
          if (!edge.IsDone()) {
            throw std::runtime_error(
                "OCCT could not build a polyline profile edge");
          }
          wire.Add(edge.Edge());
        }
        break;
      }
      default:
        throw std::runtime_error("unknown profile curve kind");
    }
  }
  if (!wire.IsDone()) {
    throw std::runtime_error("OCCT could not build the analytic profile wire");
  }
  return wire.Wire();
}

std::pair<std::size_t, std::size_t> region_range(const FfiJob& job,
                                                  std::size_t region_index) {
  if (region_index + 1 >= job.region_offsets.size()) {
    throw std::runtime_error("profile region buffer is malformed");
  }
  const std::size_t begin = job.region_offsets[region_index];
  const std::size_t end = job.region_offsets[region_index + 1];
  if (end <= begin || end >= job.profile_offsets.size()) {
    throw std::runtime_error("profile region is empty or out of range");
  }
  return {begin, end};
}

TopoDS_Face make_profile_face(const FfiJob& job, std::size_t profile_index,
                              const SectionTransform* transform = nullptr) {
  const TopoDS_Wire outer = make_profile_wire(job, profile_index, transform);
  BRepBuilderAPI_MakeFace face(outer, true);
  if (!face.IsDone()) {
    throw std::runtime_error("OCCT could not build a profile face");
  }
  return face.Face();
}

gp_Ax2 profile_fixed_axes(const FfiJob& job, std::size_t profile_index) {
  if (profile_index + 1 >= job.profile_offsets.size()) {
    throw std::runtime_error("profile offset buffer is malformed");
  }
  const std::size_t begin = job.profile_offsets[profile_index];
  const std::size_t end = job.profile_offsets[profile_index + 1];
  if (end < begin + 3) {
    throw std::runtime_error("fixed sweep orientation needs three profile points");
  }
  const gp_Pnt origin = point_at(job, begin);
  gp_Vec x(origin, point_at(job, begin + 1));
  if (x.SquareMagnitude() < 1e-18) {
    throw std::runtime_error("fixed sweep profile axis is degenerate");
  }
  gp_Vec normal;
  bool found_normal = false;
  for (std::size_t index = begin + 2; index < end; ++index) {
    normal = x.Crossed(gp_Vec(origin, point_at(job, index)));
    if (normal.SquareMagnitude() >= 1e-18) {
      found_normal = true;
      break;
    }
  }
  if (!found_normal) {
    throw std::runtime_error("fixed sweep profile plane is degenerate");
  }
  return gp_Ax2(origin, gp_Dir(normal), gp_Dir(x));
}

void configure_pipe(const FfiJob& job, BRepOffsetAPI_MakePipeShell& pipe,
                    std::size_t profile_index, bool allow_guide) {
  if (job.orientation == 0) {
    pipe.SetMode(false);
  } else if (job.orientation == 1) {
    pipe.SetMode(true);
  } else if (job.orientation == 2) {
    pipe.SetMode(profile_fixed_axes(job, profile_index));
  } else {
    throw std::runtime_error("unknown sweep orientation");
  }
  if (job.transition == 0) {
    pipe.SetTransitionMode(BRepBuilderAPI_Transformed);
  } else if (job.transition == 1) {
    pipe.SetTransitionMode(BRepBuilderAPI_RightCorner);
  } else if (job.transition == 2) {
    pipe.SetTransitionMode(BRepBuilderAPI_RoundCorner);
  } else {
    throw std::runtime_error("unknown sweep transition");
  }
  pipe.SetForceApproxC1(job.force_c1);
  if (allow_guide && !job.guide_curve_kinds.empty()) {
    const TopoDS_Wire guide =
        make_curve_wire(job.guide_curve_kinds, job.guide_curve_point_offsets,
                        job.guide_curve_points, "guide rail");
    pipe.SetMode(guide, true, BRepFill_ContactOnBorder);
  }
}

TopoDS_Shape make_exact_face_tool(const FfiJob& job,
                                  const TopoDS_Face& source_face) {
  BRepAdaptor_Surface surface(source_face, true);
  if (surface.GetType() != GeomAbs_Plane) {
    throw std::runtime_error("Extrude source face is not planar");
  }
  gp_Vec direction(job.normal_x, job.normal_y, job.normal_z);
  if (direction.SquareMagnitude() < 1e-18) {
    throw std::runtime_error("extrude normal is degenerate");
  }
  direction.Normalize();

  auto transformed_shape = [&](const TopoDS_Shape& shape, double offset,
                               double scale, const gp_Pnt& center) {
    if (!std::isfinite(scale) || scale <= 1e-6) {
      throw std::runtime_error("taper collapses or inverts the planar face");
    }
    const gp_Vec translation = direction.Multiplied(offset);
    gp_Trsf transform;



    transform.SetValues(
        scale, 0.0, 0.0,
        center.X() * (1.0 - scale) + translation.X(),
        0.0, scale, 0.0,
        center.Y() * (1.0 - scale) + translation.Y(),
        0.0, 0.0, scale,
        center.Z() * (1.0 - scale) + translation.Z());
    BRepBuilderAPI_Transform transformed(shape, transform, true);
    if (!transformed.IsDone() || transformed.Shape().IsNull()) {
      throw std::runtime_error("OCCT could not transform the planar face");
    }
    return transformed.Shape();
  };

  if (std::abs(job.taper_angle_deg) < 1e-12) {
    GProp_GProps properties;
    BRepGProp::SurfaceProperties(source_face, properties);
    const TopoDS_Shape shifted = transformed_shape(
        source_face, job.start_offset, 1.0, properties.CentreOfMass());
    const TopoDS_Face start_face = TopoDS::Face(shifted);
    gp_Vec prism_direction = direction;
    prism_direction.Multiply(job.end_offset - job.start_offset);
    BRepPrimAPI_MakePrism prism(start_face, prism_direction, true, true);
    if (!prism.IsDone() || prism.Shape().IsNull()) {
      throw std::runtime_error("OCCT exact-face prism construction failed");
    }
    return prism.Shape();
  }

  GProp_GProps properties;
  BRepGProp::SurfaceProperties(source_face, properties);
  const gp_Pnt center = properties.CentreOfMass();
  double radius_sum = 0.0;
  std::size_t radius_count = 0;
  for (TopExp_Explorer vertices(source_face, TopAbs_VERTEX); vertices.More();
       vertices.Next()) {
    radius_sum += center.Distance(BRep_Tool::Pnt(TopoDS::Vertex(vertices.Current())));
    ++radius_count;
  }
  if (radius_count == 0) {
    throw std::runtime_error("planar face has no boundary vertices");
  }
  const double reference_radius =
      std::max(radius_sum / static_cast<double>(radius_count), 1e-6);
  const double tangent = std::tan(job.taper_angle_deg * kPi / 180.0);
  const auto scale_at = [&](double offset) {
    return 1.0 + tangent * offset / reference_radius;
  };

  const TopoDS_Wire outer = BRepTools::OuterWire(source_face);
  if (outer.IsNull()) {
    throw std::runtime_error("planar face has no outer boundary wire");
  }
  std::vector<TopoDS_Wire> wires{outer};
  for (TopExp_Explorer explorer(source_face, TopAbs_WIRE); explorer.More();
       explorer.Next()) {
    const TopoDS_Wire wire = TopoDS::Wire(explorer.Current());
    if (!wire.IsSame(outer)) {
      wires.push_back(wire);
    }
  }

  auto loft_wire = [&](const TopoDS_Wire& wire) {
    const TopoDS_Wire first = TopoDS::Wire(transformed_shape(
        wire, job.start_offset, scale_at(job.start_offset), center));
    const TopoDS_Wire last = TopoDS::Wire(transformed_shape(
        wire, job.end_offset, scale_at(job.end_offset), center));
    BRepOffsetAPI_ThruSections loft(true, true, 1e-7);
    loft.CheckCompatibility(true);
    loft.AddWire(first);
    loft.AddWire(last);
    loft.Build(Message_ProgressRange());
    if (!loft.IsDone() || loft.Shape().IsNull()) {
      throw std::runtime_error("OCCT exact-wire tapered loft failed");
    }
    return loft.Shape();
  };
  TopoDS_Shape result = loft_wire(wires.front());
  for (std::size_t index = 1; index < wires.size(); ++index) {
    const TopoDS_Shape hole = loft_wire(wires[index]);
    BRepAlgoAPI_Cut cut(result, hole, Message_ProgressRange());
    if (!cut.IsDone() || cut.Shape().IsNull()) {
      throw std::runtime_error("OCCT could not preserve a tapered face hole");
    }
    result = cut.Shape();
  }
  return result;
}

TopoDS_Shape make_tool(const FfiJob& job, std::size_t region_index) {
  const auto wire_range = region_range(job, region_index);
  const std::size_t wire_begin = wire_range.first;
  const std::size_t wire_end = wire_range.second;
  const std::size_t begin = job.profile_offsets[wire_begin];
  const std::size_t end = job.profile_offsets[wire_begin + 1];
  if (end <= begin + 2 || end * 3 > job.points.size()) {
    throw std::runtime_error("profile offset is out of range");
  }

  if (job.kind == 1) {
    const gp_Vec direction(job.axis_direction_x, job.axis_direction_y,
                           job.axis_direction_z);
    if (direction.SquareMagnitude() < 1e-18) {
      throw std::runtime_error("revolve axis is degenerate");
    }
    const gp_Ax1 axis(
        gp_Pnt(job.axis_origin_x, job.axis_origin_y, job.axis_origin_z),
        gp_Dir(direction));
    auto revolve_wire = [&](std::size_t wire_index) {
      const TopoDS_Face face = make_profile_face(job, wire_index);
      BRepPrimAPI_MakeRevol revolve(face, axis, job.angle_rad, true);
      if (!revolve.IsDone()) {
        throw std::runtime_error("OCCT revolve construction failed");
      }
      return revolve.Shape();
    };
    TopoDS_Shape result = revolve_wire(wire_begin);
    for (std::size_t wire_index = wire_begin + 1; wire_index < wire_end;
         ++wire_index) {
      const TopoDS_Shape cutter = revolve_wire(wire_index);
      BRepAlgoAPI_Cut cut(result, cutter, Message_ProgressRange());
      if (!cut.IsDone() || cut.Shape().IsNull()) {
        throw std::runtime_error("OCCT could not revolve a profile hole");
      }
      result = cut.Shape();
    }
    return result;
  }
  if (job.kind == 2) {
    const TopoDS_Wire path_wire =
        make_curve_wire(job.path_curve_kinds, job.path_curve_point_offsets,
                        job.path_curve_points, "sweep path");
    auto sweep_wire = [&](std::size_t wire_index) {
      const TopoDS_Wire profile = make_profile_wire(job, wire_index);
      BRepOffsetAPI_MakePipeShell pipe(path_wire);
      configure_pipe(job, pipe, wire_index, wire_index == wire_begin);
      pipe.Add(profile, false, false);
      pipe.Build(Message_ProgressRange());
      if (!pipe.IsDone()) {
        throw std::runtime_error("OCCT sweep construction failed");
      }
      if (!pipe.MakeSolid()) {
        throw std::runtime_error("OCCT sweep could not close into a solid");
      }
      GProp_GProps sweep_properties;
      BRepGProp::VolumeProperties(pipe.Shape(), sweep_properties);
      if (!BRepCheck_Analyzer(pipe.Shape(), true, false).IsValid() ||
          !std::isfinite(sweep_properties.Mass()) ||
          std::abs(sweep_properties.Mass()) <= 1e-9) {
        throw std::runtime_error(
            "Sweep did not produce a valid solid. Place the profile across "
            "the path at its start and avoid a self-intersecting sweep.");
      }
      return pipe.Shape();
    };
    TopoDS_Shape result = sweep_wire(wire_begin);
    for (std::size_t wire_index = wire_begin + 1; wire_index < wire_end;
         ++wire_index) {
      const TopoDS_Shape cutter = sweep_wire(wire_index);
      BRepAlgoAPI_Cut cut(result, cutter, Message_ProgressRange());
      if (!cut.IsDone() || cut.Shape().IsNull()) {
        throw std::runtime_error("OCCT could not sweep a profile hole");
      }
      result = cut.Shape();
    }
    return result;
  }
  if (job.kind != 0 && job.kind != 4) {
    throw std::runtime_error("unknown solid job kind");
  }

  gp_Pnt centroid(0.0, 0.0, 0.0);
  for (std::size_t index = begin; index < end; ++index) {
    const gp_Pnt point = point_at(job, index);
    centroid.SetX(centroid.X() + point.X());
    centroid.SetY(centroid.Y() + point.Y());
    centroid.SetZ(centroid.Z() + point.Z());
  }
  const double count = static_cast<double>(end - begin);
  centroid.SetX(centroid.X() / count);
  centroid.SetY(centroid.Y() / count);
  centroid.SetZ(centroid.Z() / count);
  double radius = 0.0;
  for (std::size_t index = begin; index < end; ++index) {
    radius += centroid.Distance(point_at(job, index));
  }
  radius = std::max(radius / count, 1e-6);

  const SectionTransform first_transform =
      section_transform(job, begin, end, job.start_offset, radius);
  const SectionTransform last_transform =
      section_transform(job, begin, end, job.end_offset, radius);
  if (std::abs(job.taper_angle_deg) < 1e-12) {
    gp_Vec direction(job.normal_x, job.normal_y, job.normal_z);
    direction.Normalize();
    direction.Multiply(job.end_offset - job.start_offset);
    auto prism_wire = [&](std::size_t wire_index) {
      const TopoDS_Face face =
          make_profile_face(job, wire_index, &first_transform);
      BRepPrimAPI_MakePrism prism(face, direction, true, true);
      if (!prism.IsDone()) {
        throw std::runtime_error("OCCT prism construction failed");
      }
      return prism.Shape();
    };
    TopoDS_Shape result = prism_wire(wire_begin);
    for (std::size_t wire_index = wire_begin + 1; wire_index < wire_end;
         ++wire_index) {
      const TopoDS_Shape cutter = prism_wire(wire_index);
      BRepAlgoAPI_Cut cut(result, cutter, Message_ProgressRange());
      if (!cut.IsDone() || cut.Shape().IsNull()) {
        throw std::runtime_error("OCCT could not extrude a profile hole");
      }
      result = cut.Shape();
    }
    return result;
  }

  auto loft_wire = [&](std::size_t wire_index) {
    const TopoDS_Wire first_wire =
        make_profile_wire(job, wire_index, &first_transform);
    const TopoDS_Wire last_wire =
        make_profile_wire(job, wire_index, &last_transform);
    BRepOffsetAPI_ThruSections loft(true, true, 1e-7);
    loft.CheckCompatibility(true);
    loft.AddWire(first_wire);
    loft.AddWire(last_wire);
    loft.Build(Message_ProgressRange());
    if (!loft.IsDone()) {
      throw std::runtime_error("OCCT tapered loft construction failed");
    }
    return loft.Shape();
  };
  TopoDS_Shape result = loft_wire(wire_begin);
  for (std::size_t wire_index = wire_begin + 1; wire_index < wire_end;
       ++wire_index) {
    const TopoDS_Shape hole = loft_wire(wire_index);
    BRepAlgoAPI_Cut cut(result, hole, Message_ProgressRange());
    if (!cut.IsDone()) {
      throw std::runtime_error("OCCT could not taper a profile hole");
    }
    result = cut.Shape();
  }
  return result;
}

TopoDS_Shape make_loft_tool(const FfiJob& job) {
  if (job.region_offsets.size() < 3) {
    throw std::runtime_error("Loft needs at least two sections");
  }
  const std::size_t section_count = job.region_offsets.size() - 1;
  const std::size_t wire_count =
      job.region_offsets[1] - job.region_offsets[0];
  for (std::size_t section = 1; section < section_count; ++section) {
    if (job.region_offsets[section + 1] - job.region_offsets[section] !=
        wire_count) {
      throw std::runtime_error(
          "Loft sections must contain the same number of profile holes");
    }
  }
  const bool guided =
      !job.path_curve_kinds.empty() || !job.guide_curve_kinds.empty();
  auto centerline_wire = [&]() {
    if (!job.path_curve_kinds.empty()) {
      return make_curve_wire(job.path_curve_kinds,
                             job.path_curve_point_offsets,
                             job.path_curve_points, "loft centerline");
    }
    std::vector<gp_Pnt> centroids;
    centroids.reserve(section_count);
    for (std::size_t section = 0; section < section_count; ++section) {
      const std::size_t profile_index = job.region_offsets[section];
      const std::size_t begin = job.profile_offsets[profile_index];
      const std::size_t end = job.profile_offsets[profile_index + 1];
      gp_Pnt centroid(0.0, 0.0, 0.0);
      for (std::size_t index = begin; index < end; ++index) {
        const gp_Pnt point = point_at(job, index);
        centroid.SetX(centroid.X() + point.X());
        centroid.SetY(centroid.Y() + point.Y());
        centroid.SetZ(centroid.Z() + point.Z());
      }
      const double count = static_cast<double>(end - begin);
      centroid.SetX(centroid.X() / count);
      centroid.SetY(centroid.Y() / count);
      centroid.SetZ(centroid.Z() / count);
      centroids.push_back(centroid);
    }
    return make_open_wire(centroids);
  };
  auto loft_wire = [&](std::size_t wire_offset) {
    if (guided) {
      const TopoDS_Wire spine = centerline_wire();
      BRepOffsetAPI_MakePipeShell loft(spine);
      loft.SetMode(false);
      loft.SetForceApproxC1(job.continuity >= 1);
      if (wire_offset == 0 && !job.guide_curve_kinds.empty()) {
        const TopoDS_Wire guide =
            make_curve_wire(job.guide_curve_kinds,
                            job.guide_curve_point_offsets,
                            job.guide_curve_points, "loft guide rail");
        loft.SetMode(guide, true, BRepFill_ContactOnBorder);
      }
      for (std::size_t section = 0; section < section_count; ++section) {
        loft.Add(make_profile_wire(
            job, job.region_offsets[section] + wire_offset), false, false);
      }
      loft.Build(Message_ProgressRange());
      if (!loft.IsDone()) {
        throw std::runtime_error("OCCT guided loft construction failed");
      }
      if (!loft.MakeSolid()) {
        throw std::runtime_error("OCCT guided loft could not close into a solid");
      }
      return loft.Shape();
    }
    BRepOffsetAPI_ThruSections loft(true, job.ruled, 1e-7);
    loft.CheckCompatibility(true);
    if (job.continuity == 0) {
      loft.SetContinuity(GeomAbs_C0);
    } else if (job.continuity == 1) {
      loft.SetContinuity(GeomAbs_G1);
    } else if (job.continuity == 2) {
      loft.SetContinuity(GeomAbs_G2);
    } else {
      throw std::runtime_error("unknown loft continuity");
    }
    for (std::size_t section = 0; section < section_count; ++section) {
      loft.AddWire(make_profile_wire(
          job, job.region_offsets[section] + wire_offset));
    }
    loft.Build(Message_ProgressRange());
    if (!loft.IsDone()) {
      throw std::runtime_error("OCCT loft construction failed");
    }
    return loft.Shape();
  };
  TopoDS_Shape result = loft_wire(0);
  for (std::size_t hole = 1; hole < wire_count; ++hole) {
    const TopoDS_Shape cutter = loft_wire(hole);
    BRepAlgoAPI_Cut cut(result, cutter, Message_ProgressRange());
    if (!cut.IsDone()) {
      throw std::runtime_error("OCCT could not loft a profile hole");
    }
    result = cut.Shape();
  }
  return result;
}

TopoDS_Shape fuse_shapes(const std::vector<TopoDS_Shape>& shapes) {
  if (shapes.empty()) {
    throw std::runtime_error("extrude contains no tool profiles");
  }
  TopoDS_Shape result = shapes.front();
  for (std::size_t index = 1; index < shapes.size(); ++index) {
    BRepAlgoAPI_Fuse fuse(result, shapes[index], Message_ProgressRange());
    if (!fuse.IsDone()) {
      throw std::runtime_error("OCCT could not combine tool profiles");
    }
    fuse.SimplifyResult(true, true, 1.0e-7);
    result = fuse.Shape();
  }
  return result;
}

void append_point(rust::Vec<double>& output, const gp_Pnt& point) {
  output.push_back(point.X());
  output.push_back(point.Y());
  output.push_back(point.Z());
}

std::vector<gp_Pnt> sample_projection_edge(const TopoDS_Edge& edge,
                                           double deflection) {
  BRepAdaptor_Curve curve(edge);
  std::vector<gp_Pnt> points;
  if (curve.GetType() == GeomAbs_Line) {
    points.push_back(curve.Value(curve.FirstParameter()));
    points.push_back(curve.Value(curve.LastParameter()));
    return points;
  }
  GCPnts_UniformDeflection discretization(
      curve, std::max(1.0e-4, deflection), true);
  if (discretization.IsDone() && discretization.NbPoints() >= 2) {
    points.reserve(discretization.NbPoints());
    for (int index = 1; index <= discretization.NbPoints(); ++index) {
      points.push_back(discretization.Value(index));
    }
    return points;
  }
  const double first = curve.FirstParameter();
  const double last = curve.LastParameter();
  constexpr int kFallbackSamples = 25;
  points.reserve(kFallbackSamples);
  for (int index = 0; index < kFallbackSamples; ++index) {
    const double parameter =
        first + (last - first) * static_cast<double>(index) /
                    static_cast<double>(kFallbackSamples - 1);
    points.push_back(curve.Value(parameter));
  }
  return points;
}

std::vector<gp_Pnt> sample_section_edge(const TopoDS_Edge& edge,
                                      double deflection, std::size_t limit,
                                      const SectionProgress& progress) {
  progress.check("curve sampling");
  BRepAdaptor_Curve curve(edge);
  std::vector<gp_Pnt> points;
  auto append = [&](const gp_Pnt& point) {
    progress.check("curve sampling");
    if (points.size() >= limit) {
      throw std::runtime_error("Section curve exceeds the native point budget");
    }
    if (!std::isfinite(point.X()) || !std::isfinite(point.Y()) || !std::isfinite(point.Z())) {
      throw std::runtime_error("Section curve contains non-finite coordinates");
    }
    points.push_back(point);
  };
  if (curve.GetType() == GeomAbs_Line) {
    append(curve.Value(curve.FirstParameter()));
    append(curve.Value(curve.LastParameter()));
    return points;
  }
  const int intervals = curve.NbIntervals(GeomAbs_C2);
  if (intervals <= 0 || static_cast<std::size_t>(intervals) >= limit) {
    throw std::runtime_error("Section curve continuity exceeds the native point budget");
  }
  TColStd_Array1OfReal parameters(1, intervals + 1);
  curve.Intervals(parameters, GeomAbs_C2);
  for (int interval = 1; interval <= intervals; ++interval) {
    CPnts_UniformDeflection samples(curve, deflection,
        parameters(interval), parameters(interval + 1), Precision::PConfusion(), true);
    for (; samples.More(); samples.Next()) {
      append(samples.Point());
    }
    if (!samples.IsAllDone()) {
      throw std::runtime_error("OCCT section curve sampling failed");
    }
  }
  return points;
}

std::vector<std::int64_t> projection_polyline_key(
    const std::vector<gp_Pnt>& points) {
  constexpr double kQuantize = 1.0e7;
  std::vector<std::int64_t> forward;
  std::vector<std::int64_t> reverse;
  forward.reserve(points.size() * 2);
  reverse.reserve(points.size() * 2);
  for (const gp_Pnt& point : points) {
    forward.push_back(static_cast<std::int64_t>(std::llround(point.X() * kQuantize)));
    forward.push_back(static_cast<std::int64_t>(std::llround(point.Y() * kQuantize)));
  }
  for (auto iterator = points.rbegin(); iterator != points.rend(); ++iterator) {
    reverse.push_back(static_cast<std::int64_t>(std::llround(iterator->X() * kQuantize)));
    reverse.push_back(static_cast<std::int64_t>(std::llround(iterator->Y() * kQuantize)));
  }
  return reverse < forward ? reverse : forward;
}

void append_projection_shape(
    const TopoDS_Shape& shape,
    double deflection,
    rust::Vec<std::uint32_t>& offsets,
    rust::Vec<double>& coordinates,
    std::set<std::vector<std::int64_t>>& seen) {
  if (shape.IsNull()) {
    return;
  }
  for (TopExp_Explorer explorer(shape, TopAbs_EDGE); explorer.More(); explorer.Next()) {
    const TopoDS_Edge edge = TopoDS::Edge(explorer.Current());
    std::vector<gp_Pnt> points = sample_projection_edge(edge, deflection);
    if (points.size() < 2) {
      continue;
    }
    const auto key = projection_polyline_key(points);
    if (!seen.insert(key).second) {
      continue;
    }
    for (const gp_Pnt& point : points) {
      coordinates.push_back(point.X());
      coordinates.push_back(point.Y());
    }
    offsets.push_back(static_cast<std::uint32_t>(coordinates.size() / 2));
  }
}

void append_section_shape(
    const TopoDS_Shape& shape,
    const gp_Vec& right,
    const gp_Vec& page_up,
    double deflection,
    rust::Vec<std::uint32_t>& offsets,
    rust::Vec<double>& coordinates,
    std::set<std::vector<std::int64_t>>& seen,
    const SectionProgress* progress = nullptr,
    std::size_t point_limit = std::numeric_limits<std::uint32_t>::max()) {
  if (shape.IsNull()) {
    return;
  }
  if (coordinates.size() / 2 > point_limit) {
    throw std::runtime_error("Section contour exceeds the native point budget before append");
  }
  constexpr double kQuantize = 1.0e7;
  for (TopExp_Explorer explorer(shape, TopAbs_EDGE); explorer.More(); explorer.Next()) {
    const TopoDS_Edge edge = TopoDS::Edge(explorer.Current());
    const std::vector<gp_Pnt> points = progress
        ? sample_section_edge(edge, deflection, point_limit - coordinates.size() / 2, *progress)
        : sample_projection_edge(edge, deflection);
    if (points.size() < 2) {
      continue;
    }
    std::vector<std::int64_t> forward;
    std::vector<std::int64_t> reverse;
    std::vector<std::array<double, 2>> projected;
    projected.reserve(points.size());
    for (const gp_Pnt& point : points) {
      const double x = point.X() * right.X() + point.Y() * right.Y() + point.Z() * right.Z();
      const double y = point.X() * page_up.X() + point.Y() * page_up.Y() + point.Z() * page_up.Z();
      constexpr double kCoordinateLimit =
          static_cast<double>(std::numeric_limits<std::int64_t>::max()) / kQuantize / 2.0;
      if (!std::isfinite(x) || !std::isfinite(y) ||
          std::abs(x) > kCoordinateLimit || std::abs(y) > kCoordinateLimit) {
        throw std::runtime_error("Section coordinates exceed the native quantization range");
      }
      projected.push_back({x, y});
      forward.push_back(static_cast<std::int64_t>(std::llround(x * kQuantize)));
      forward.push_back(static_cast<std::int64_t>(std::llround(y * kQuantize)));
    }
    for (auto iterator = projected.rbegin(); iterator != projected.rend(); ++iterator) {
      reverse.push_back(static_cast<std::int64_t>(std::llround((*iterator)[0] * kQuantize)));
      reverse.push_back(static_cast<std::int64_t>(std::llround((*iterator)[1] * kQuantize)));
    }
    const auto& key = reverse < forward ? reverse : forward;
    if (!seen.insert(key).second) {
      continue;
    }
    for (const auto& point : projected) {
      coordinates.push_back(point[0]);
      coordinates.push_back(point[1]);
    }
    offsets.push_back(static_cast<std::uint32_t>(coordinates.size() / 2));
  }
}

void append_vec(rust::Vec<float>& output, const gp_Vec& value) {
  output.push_back(static_cast<float>(value.X()));
  output.push_back(static_cast<float>(value.Y()));
  output.push_back(static_cast<float>(value.Z()));
}

static gp_Dir parametric_plane_normal(const gp_Pln& plane) {
  const gp_Ax3 axes = plane.Position();
  // Revolved planes may use an indirect Ax3. Its main Direction then opposes
  // the surface's U/V cross product, which defines the actual surface normal.
  return gp_Dir(gp_Vec(axes.XDirection()).Crossed(gp_Vec(axes.YDirection())));
}

void append_plane(rust::Vec<double>& output, const TopoDS_Face& face) {
  BRepAdaptor_Surface surface(face, true);
  if (surface.GetType() != GeomAbs_Plane) {
    for (int index = 0; index < 13; ++index) {
      output.push_back(0.0);
    }
    return;
  }
  const gp_Pln plane = surface.Plane();
  const gp_Ax3 axes = plane.Position();
  gp_Dir normal = parametric_plane_normal(plane);
  gp_Dir u = axes.XDirection();
  if (face.Orientation() == TopAbs_REVERSED) {
    normal.Reverse();
  }
  gp_Vec v = gp_Vec(normal).Crossed(gp_Vec(u));
  v.Normalize();
  output.push_back(1.0);
  append_point(output, axes.Location());
  output.push_back(u.X());
  output.push_back(u.Y());
  output.push_back(u.Z());
  output.push_back(v.X());
  output.push_back(v.Y());
  output.push_back(v.Z());
  output.push_back(normal.X());
  output.push_back(normal.Y());
  output.push_back(normal.Z());
}

void append_cylinder(rust::Vec<double>& output, const TopoDS_Face& face) {
  BRepAdaptor_Surface surface(face, true);
  if (surface.GetType() != GeomAbs_Cylinder) {
    for (int index = 0; index < 11; ++index) {
      output.push_back(0.0);
    }
    return;
  }
  const gp_Cylinder cylinder = surface.Cylinder();
  const gp_Ax3 axes = cylinder.Position();
  output.push_back(1.0);
  append_point(output, axes.Location());
  output.push_back(axes.Direction().X());
  output.push_back(axes.Direction().Y());
  output.push_back(axes.Direction().Z());
  output.push_back(axes.XDirection().X());
  output.push_back(axes.XDirection().Y());
  output.push_back(axes.XDirection().Z());
  output.push_back(cylinder.Radius());
}

void append_circle(rust::Vec<double>& output, const TopoDS_Edge& edge,
                   const BRepAdaptor_Curve& curve) {
  if (curve.GetType() != GeomAbs_Circle) {
    for (int index = 0; index < 12; ++index) {
      output.push_back(0.0);
    }
    return;
  }
  const gp_Circ circle = curve.Circle();
  const gp_Ax2 axes = circle.Position();
  const double parameter_span =
      std::abs(curve.LastParameter() - curve.FirstParameter());
  const bool closed = edge.Closed() || std::abs(parameter_span - kTau) <= 1.0e-7;
  output.push_back(1.0);
  append_point(output, axes.Location());
  output.push_back(axes.Direction().X());
  output.push_back(axes.Direction().Y());
  output.push_back(axes.Direction().Z());
  output.push_back(axes.XDirection().X());
  output.push_back(axes.XDirection().Y());
  output.push_back(axes.XDirection().Z());
  output.push_back(circle.Radius());
  output.push_back(closed ? 1.0 : 0.0);
}

struct PlanarFaceSignature {
  bool valid = false;
  gp_Pnt centroid;
  gp_Dir normal;
  double area = 0.0;
  double perimeter = 0.0;
  std::uint32_t wire_count = 0;
  std::uint32_t edge_count = 0;
};

PlanarFaceSignature planar_face_signature(const TopoDS_Face& face) {
  PlanarFaceSignature signature;
  BRepAdaptor_Surface surface(face, true);
  if (surface.GetType() != GeomAbs_Plane) {
    return signature;
  }

  GProp_GProps surface_properties;
  BRepGProp::SurfaceProperties(face, surface_properties, false, false);
  GProp_GProps edge_properties;
  BRepGProp::LinearProperties(face, edge_properties, false, false);
  TopTools_IndexedMapOfShape wires;
  TopTools_IndexedMapOfShape edges;
  TopExp::MapShapes(face, TopAbs_WIRE, wires);
  TopExp::MapShapes(face, TopAbs_EDGE, edges);

  gp_Dir normal = parametric_plane_normal(surface.Plane());
  if (face.Orientation() == TopAbs_REVERSED) {
    normal.Reverse();
  }
  signature.valid = true;
  signature.centroid = surface_properties.CentreOfMass();
  signature.normal = normal;
  signature.area = std::abs(surface_properties.Mass());
  signature.perimeter = std::abs(edge_properties.Mass());
  signature.wire_count = static_cast<std::uint32_t>(wires.Extent());
  signature.edge_count = static_cast<std::uint32_t>(edges.Extent());
  return signature;
}

void append_face_signature(rust::Vec<double>& output, const TopoDS_Face& face) {
  const PlanarFaceSignature signature = planar_face_signature(face);
  if (!signature.valid) {
    for (int index = 0; index < 8; ++index) {
      output.push_back(0.0);
    }
    return;
  }
  output.push_back(1.0);
  append_point(output, signature.centroid);
  output.push_back(signature.area);
  output.push_back(signature.perimeter);
  output.push_back(static_cast<double>(signature.wire_count));
  output.push_back(static_cast<double>(signature.edge_count));
}

bool signature_scalar_matches(double actual, double expected) {
  const double scale = std::max({1.0, std::abs(actual), std::abs(expected)});
  return std::abs(actual - expected) <= scale * 1.0e-6;
}

bool planar_face_signature_matches(
    const PlanarFaceSignature& actual,
    const rust::Vec<double>& expected) {
  if (!actual.valid || expected.size() != 10) {
    return false;
  }
  const gp_Pnt expected_centroid(expected[0], expected[1], expected[2]);
  const gp_Vec expected_normal(expected[3], expected[4], expected[5]);
  if (expected_normal.SquareMagnitude() <= 1.0e-18) {
    return false;
  }
  const double length_scale = std::max(
      {1.0, std::sqrt(std::max(actual.area, 0.0)), actual.perimeter});
  if (actual.centroid.Distance(expected_centroid) > length_scale * 1.0e-6) {
    return false;
  }
  gp_Vec normalized_expected = expected_normal;
  normalized_expected.Normalize();
  if (gp_Vec(actual.normal).Dot(normalized_expected) < 1.0 - 1.0e-7) {
    return false;
  }
  return signature_scalar_matches(actual.area, expected[6]) &&
         signature_scalar_matches(actual.perimeter, expected[7]) &&
         actual.wire_count == static_cast<std::uint32_t>(std::llround(expected[8])) &&
         actual.edge_count == static_cast<std::uint32_t>(std::llround(expected[9]));
}

TopoDS_Face resolve_planar_face_reference(
    const TopoDS_Shape& body,
    const FfiJob& job) {
  if (job.source_face_signature.size() != 10) {
    throw std::runtime_error(
        "Extrude source face has no validated topology signature; reselect it");
  }
  TopTools_IndexedMapOfShape faces;
  TopExp::MapShapes(body, TopAbs_FACE, faces);
  std::vector<TopoDS_Face> matches;
  for (int index = 1; index <= faces.Extent(); ++index) {
    const TopoDS_Face face = TopoDS::Face(faces.FindKey(index));
    if (planar_face_signature_matches(
            planar_face_signature(face), job.source_face_signature)) {
      matches.push_back(face);
    }
  }
  if (matches.empty()) {
    throw std::runtime_error(
        "referenced Extrude source face changed or no longer exists");
  }
  if (matches.size() != 1) {
    throw std::runtime_error(
        "referenced Extrude source face is ambiguous after topology change");
  }
  return matches.front();
}









struct PrismaticCorner {
  gp_Pnt start;
  gp_Vec along;
  gp_Dir into_first;
  gp_Dir into_second;
  double first_width;
  double second_width;
  bool concave;
};

bool point_in_face(const TopoDS_Face& face, const gp_Pnt& point, double tolerance) {
  BRepClass_FaceClassifier classifier(face, point, tolerance);
  const TopAbs_State state = classifier.State();
  return state == TopAbs_IN || state == TopAbs_ON;
}


double wall_width(const TopoDS_Face& face, const gp_Pnt& origin, const gp_Dir& direction,
                  double probe, double tolerance) {
  const auto inside = [&](double distance) {
    return point_in_face(face, origin.Translated(gp_Vec(direction) * distance), tolerance);
  };
  if (inside(probe)) {
    return probe;
  }
  double low = 0.0;
  double high = probe;
  for (int iteration = 0; iteration < 48; ++iteration) {
    const double middle = 0.5 * (low + high);
    if (inside(middle)) {
      low = middle;
    } else {
      high = middle;
    }
  }
  return low;
}

std::optional<PrismaticCorner> prismatic_corner(
    const TopoDS_Edge& edge,
    const TopTools_IndexedDataMapOfShapeListOfShape& edge_faces,
    const TopTools_IndexedDataMapOfShapeListOfShape& vertex_faces,
    double probe) {
  if (BRepAdaptor_Curve(edge).GetType() != GeomAbs_Line || !edge_faces.Contains(edge)) {
    return std::nullopt;
  }
  const TopTools_ListOfShape& adjacent = edge_faces.FindFromKey(edge);
  if (adjacent.Extent() != 2) {
    return std::nullopt;
  }
  TopoDS_Face faces[2];
  gp_Dir normals[2];
  int count = 0;
  for (TopTools_ListIteratorOfListOfShape iterator(adjacent); iterator.More();
       iterator.Next(), ++count) {
    faces[count] = TopoDS::Face(iterator.Value());
    BRepAdaptor_Surface surface(faces[count], true);
    if (surface.GetType() != GeomAbs_Plane) {
      return std::nullopt;
    }
    gp_Dir normal = parametric_plane_normal(surface.Plane());
    if (faces[count].Orientation() == TopAbs_REVERSED) {
      normal.Reverse();
    }
    normals[count] = normal;
  }
  TopoDS_Vertex first, last;
  TopExp::Vertices(edge, first, last, true);
  if (first.IsNull() || last.IsNull()) {
    return std::nullopt;
  }
  const gp_Pnt start = BRep_Tool::Pnt(first);
  const gp_Vec along(start, BRep_Tool::Pnt(last));
  if (along.SquareMagnitude() < 1.0e-12) {
    return std::nullopt;
  }
  const gp_Dir direction(along);


  for (const TopoDS_Vertex& vertex : {first, last}) {
    if (!vertex_faces.Contains(vertex)) {
      return std::nullopt;
    }
    for (TopTools_ListIteratorOfListOfShape iterator(vertex_faces.FindFromKey(vertex));
         iterator.More(); iterator.Next()) {
      const TopoDS_Face face = TopoDS::Face(iterator.Value());
      if (face.IsSame(faces[0]) || face.IsSame(faces[1])) {
        continue;
      }
      BRepAdaptor_Surface surface(face, true);
      if (surface.GetType() != GeomAbs_Plane ||
          std::abs(surface.Plane().Axis().Direction().Dot(direction)) < 1.0 - 1.0e-6) {
        return std::nullopt;
      }
    }
  }
  const gp_Pnt middle = start.Translated(along * 0.5);
  gp_Dir into[2];
  double widths[2];
  for (int side = 0; side < 2; ++side) {
    const double tolerance = std::max(BRep_Tool::Tolerance(faces[side]), 1.0e-7);
    const double step = std::max(probe * 1.0e-3, tolerance * 10.0);
    const gp_Dir candidate = normals[side].Crossed(direction);
    if (point_in_face(faces[side], middle.Translated(gp_Vec(candidate) * step), tolerance)) {
      into[side] = candidate;
    } else if (point_in_face(faces[side], middle.Translated(gp_Vec(candidate.Reversed()) * step),
                             tolerance)) {
      into[side] = candidate.Reversed();
    } else {
      return std::nullopt;
    }
    widths[side] = wall_width(faces[side], middle, into[side], probe, tolerance);
  }

  const bool concave = into[1].Dot(normals[0]) > 0.0;
  return PrismaticCorner{start, along, into[0], into[1], widths[0], widths[1], concave};
}

std::string format_millimetres(double value) {
  std::ostringstream text;
  text.precision(4);
  text << value;
  return text.str();
}

TopoDS_Shape blend_prismatic_corners(const TopoDS_Shape& shape,
                                     const std::vector<TopoDS_Edge>& edges, double size,
                                     bool chamfer, const char* generic_failure) {
  TopTools_IndexedDataMapOfShapeListOfShape edge_faces;
  TopTools_IndexedDataMapOfShapeListOfShape vertex_faces;
  TopExp::MapShapesAndUniqueAncestors(shape, TopAbs_EDGE, TopAbs_FACE, edge_faces, false);
  TopExp::MapShapesAndUniqueAncestors(shape, TopAbs_VERTEX, TopAbs_FACE, vertex_faces, false);
  const double pi = std::acos(-1.0);
  std::vector<std::pair<TopoDS_Shape, bool>> prisms;
  for (const TopoDS_Edge& edge : edges) {

    const std::optional<PrismaticCorner> corner =
        prismatic_corner(edge, edge_faces, vertex_faces, size * 4.0);
    if (!corner) {
      throw std::runtime_error(generic_failure);
    }
    const double angle = corner->into_first.Angle(corner->into_second);
    if (angle < 1.0e-3 || angle > pi - 1.0e-3) {
      throw std::runtime_error(generic_failure);
    }

    const double reach = chamfer ? size : size / std::tan(angle * 0.5);
    const double narrowest = std::min(corner->first_width, corner->second_width);
    if (reach > narrowest + 1.0e-6) {
      throw std::runtime_error("A " + format_millimetres(size) + " mm " +
                               (chamfer ? "chamfer" : "fillet") + " reaches past the " +
                               format_millimetres(narrowest) +
                               " mm wall beside the selected edge");
    }
    const gp_Pnt first_tangent = corner->start.Translated(gp_Vec(corner->into_first) * reach);
    const gp_Pnt second_tangent = corner->start.Translated(gp_Vec(corner->into_second) * reach);
    BRepBuilderAPI_MakeWire wire;
    wire.Add(BRepBuilderAPI_MakeEdge(corner->start, first_tangent));
    if (chamfer) {
      wire.Add(BRepBuilderAPI_MakeEdge(first_tangent, second_tangent));
    } else {
      gp_Vec bisector = gp_Vec(corner->into_first) + gp_Vec(corner->into_second);
      bisector.Normalize();
      const gp_Pnt centre = corner->start.Translated(bisector * (size / std::sin(angle * 0.5)));
      const gp_Pnt crown = centre.Translated(bisector * -size);
      wire.Add(BRepBuilderAPI_MakeEdge(
          GC_MakeArcOfCircle(first_tangent, crown, second_tangent).Value()));
    }
    wire.Add(BRepBuilderAPI_MakeEdge(second_tangent, corner->start));
    if (!wire.IsDone()) {
      throw std::runtime_error(generic_failure);
    }
    BRepBuilderAPI_MakeFace section(wire.Wire(), true);
    if (!section.IsDone()) {
      throw std::runtime_error(generic_failure);
    }
    BRepPrimAPI_MakePrism prism(section.Face(), corner->along);
    if (!prism.IsDone()) {
      throw std::runtime_error(generic_failure);
    }
    prisms.emplace_back(prism.Shape(), corner->concave);
  }
  TopoDS_Shape result = shape;
  for (const auto& [prism, concave] : prisms) {
    TopTools_ListOfShape arguments;
    arguments.Append(result);
    TopTools_ListOfShape tools;
    tools.Append(prism);
    std::unique_ptr<BRepAlgoAPI_BooleanOperation> operation;
    if (concave) {
      operation = std::make_unique<BRepAlgoAPI_Fuse>();
    } else {
      operation = std::make_unique<BRepAlgoAPI_Cut>();
    }
    operation->SetArguments(arguments);
    operation->SetTools(tools);

    operation->SetFuzzyValue(1.0e-6);
    operation->Build(Message_ProgressRange());
    if (!operation->IsDone() || operation->HasErrors() || operation->Shape().IsNull()) {
      throw std::runtime_error(generic_failure);
    }
    result = operation->Shape();
  }


  ShapeUpgrade_UnifySameDomain unify(result, true, true, false);
  unify.Build();
  result = unify.Shape();
  BRepLib::EncodeRegularity(result);
  int solids = 0;
  for (TopExp_Explorer explorer(result, TopAbs_SOLID); explorer.More(); explorer.Next()) {
    ++solids;
  }
  if (solids != 1 || !BRepCheck_Analyzer(result).IsValid()) {
    throw std::runtime_error(generic_failure);
  }
  return result;
}

}

class Kernel::Impl {
 public:
  std::map<std::uint64_t, TopoDS_Shape> bodies;
  std::set<std::uint64_t> imported_display_bodies;
};

Kernel::Kernel() : impl_(std::make_unique<Impl>()) {}
Kernel::~Kernel() = default;

void Kernel::reset() {
  impl_->bodies.clear();
  impl_->imported_display_bodies.clear();
}

void Kernel::apply_job(const FfiJob& job) {
  // Generated/replaced bodies do not inherit the original STEP display policy.
  for (const auto body_id : job.result_body_ids)
    impl_->imported_display_bodies.erase(body_id);
  if (job.kind == 12) {
    if (job.result_body_ids.size() != 1 || job.step_data.empty()) {
      throw std::runtime_error("STEP import buffers are malformed");
    }
    std::string content;
    content.reserve(job.step_data.size());
    for (const std::uint8_t byte : job.step_data) {
      content.push_back(static_cast<char>(byte));
    }
    std::istringstream stream(content);
    STEPControl_Reader reader;
    if (reader.ReadStream("import.step", stream) != IFSelect_RetDone) {
      throw std::runtime_error("OCCT could not read the STEP stream");
    }
    if (reader.TransferRoots(Message_ProgressRange()) <= 0) {
      throw std::runtime_error("STEP file did not contain transferable shapes");
    }
    const TopoDS_Shape shape = reader.OneShape();
    if (shape.IsNull()) {
      throw std::runtime_error("STEP import produced a null shape");
    }
    impl_->bodies[job.result_body_ids[0]] = shape;
    impl_->imported_display_bodies.insert(job.result_body_ids[0]);
    return;
  }
  if (job.kind == 5 || job.kind == 6) {
    if (job.target_body_ids.size() != 1 || job.edge_indices.empty()) {
      throw std::runtime_error("edge refinement needs one body and at least one edge");
    }
    auto found = impl_->bodies.find(job.target_body_ids[0]);
    if (found == impl_->bodies.end()) {
      throw std::runtime_error("edge refinement target body is missing");
    }
    TopTools_IndexedMapOfShape edge_map;
    TopExp::MapShapes(found->second, TopAbs_EDGE, edge_map);
    std::vector<TopoDS_Edge> selected;
    for (const std::uint32_t index : job.edge_indices) {
      if (index >= static_cast<std::uint32_t>(edge_map.Extent())) {
        throw std::runtime_error(job.kind == 5 ? "referenced fillet edge no longer exists"
                                               : "referenced chamfer edge no longer exists");
      }
      selected.push_back(TopoDS::Edge(edge_map.FindKey(index + 1)));
    }



    if (job.kind == 5) {
      BRepFilletAPI_MakeFillet fillet(found->second);
      for (const TopoDS_Edge& edge : selected) {
        fillet.Add(job.radius, edge);
      }
      fillet.Build(Message_ProgressRange());
      found->second = fillet.IsDone()
                          ? fillet.Shape()
                          : blend_prismatic_corners(
                                found->second, selected, job.radius, false,
                                "OCCT could not build the selected solid fillet");
    } else {
      BRepFilletAPI_MakeChamfer chamfer(found->second);
      for (const TopoDS_Edge& edge : selected) {
        chamfer.Add(job.radius, edge);
      }
      chamfer.Build(Message_ProgressRange());
      found->second = chamfer.IsDone()
                          ? chamfer.Shape()
                          : blend_prismatic_corners(
                                found->second, selected, job.radius, true,
                                "OCCT could not build the selected solid chamfer");
    }
    impl_->imported_display_bodies.erase(job.target_body_ids[0]);
    return;
  }
  if (job.kind == 7) {
    if (job.target_body_ids.size() != 1 || job.diameter <= 0.0 ||
        job.end_offset <= 0.0) {
      throw std::runtime_error("hole parameters are malformed");
    }
    if (job.thread_mode > 0 &&
        (job.thread_nominal_diameter <= job.diameter ||
         job.thread_pitch <= 0.0 || job.thread_depth < 0.0)) {
      throw std::runtime_error("thread parameters are malformed");
    }
    if (job.thread_mode == 2 &&
        (job.thread_major_diameter <= job.thread_pitch_diameter ||
         job.thread_pitch_diameter <= job.thread_minor_diameter ||
         job.thread_minor_diameter <= 0.0)) {
      throw std::runtime_error(
          "modeled thread tolerance limits are malformed");
    }
    auto found = impl_->bodies.find(job.target_body_ids[0]);
    if (found == impl_->bodies.end()) {
      throw std::runtime_error("hole target body is missing");
    }
    gp_Vec direction(job.axis_direction_x, job.axis_direction_y,
                     job.axis_direction_z);
    if (direction.SquareMagnitude() < 1e-18) {
      throw std::runtime_error("hole direction is degenerate");
    }
    direction.Normalize();
    const double overlap = 1e-4;
    const gp_Pnt support(job.axis_origin_x, job.axis_origin_y,
                         job.axis_origin_z);
    const gp_Pnt start = support.Translated(direction.Multiplied(-overlap));
    const gp_Ax2 axis(start, gp_Dir(direction));
    const double hole_depth =
        job.through_all
            ? bounded_through_depth(found->second, job.thread_pitch * 2.0)
            : job.end_offset;


    const double finished_hole_diameter =
        job.thread_mode == 2 ? job.thread_minor_diameter : job.diameter;
    // Entrance overlap clears the support face; a blind stop must remain
    // at the requested depth measured from that face. Through cuts retain
    // their existing overlap at both ends.
    BRepPrimAPI_MakeCylinder main_cylinder(
        axis, finished_hole_diameter * 0.5,
        hole_depth + overlap * (job.through_all ? 2.0 : 1.0));
    TopoDS_Shape cutter = main_cylinder.Shape();
    std::vector<TopoDS_Shape> thread_cutters;
    if (job.hole_style == 1) {
      BRepPrimAPI_MakeCylinder counterbore(
          axis, job.secondary_diameter * 0.5,
          job.secondary_depth + overlap * (job.through_all ? 2.0 : 1.0));
      BRepAlgoAPI_Fuse fuse(cutter, counterbore.Shape(),
                            Message_ProgressRange());
      if (!fuse.IsDone()) {
        throw std::runtime_error("OCCT could not build the counterbore cutter");
      }
      cutter = fuse.Shape();
    } else if (job.hole_style == 2) {
      const double large_radius = job.secondary_diameter * 0.5;
      const double small_radius = finished_hole_diameter * 0.5;
      const double half_angle = job.hole_angle_deg * kPi / 360.0;
      const double sink_depth = (large_radius - small_radius) / std::tan(half_angle);
      if (!std::isfinite(sink_depth) || sink_depth <= 0.0) {
        throw std::runtime_error("countersink dimensions are invalid");
      }



      BRepPrimAPI_MakeCone countersink(axis, large_radius + overlap * std::tan(half_angle), small_radius,
                                       sink_depth + overlap);
      BRepAlgoAPI_Fuse fuse(cutter, countersink.Shape(),
                            Message_ProgressRange());
      if (!fuse.IsDone()) {
        throw std::runtime_error("OCCT could not build the countersink cutter");
      }
      cutter = fuse.Shape();
    }
    if (job.thread_mode == 2) {
      const bool full_thread_depth = job.thread_depth <= 0.0;
      const double available_thread_depth =
          job.through_all
              ? bounded_directional_depth(found->second, support, direction)
              : hole_depth;
      const double requested_thread_depth =
          full_thread_depth
              ? available_thread_depth
              : std::min(job.thread_depth, available_thread_depth);


      const gp_Ax2 thread_axis(support, gp_Dir(direction), axis.XDirection());
      thread_cutters = job.thread_form == 1 ? make_rounded_thread_cutters(
          thread_axis, job.thread_major_diameter, job.thread_minor_diameter,
          job.thread_pitch, job.thread_corner_radius, job.thread_axial_clearance,
          requested_thread_depth, job.thread_left_hand, true) : make_internal_thread_cutters(
          thread_axis, job.thread_major_diameter,
          job.thread_pitch_diameter, job.thread_minor_diameter,
          job.thread_pitch, requested_thread_depth, job.thread_left_hand);
      if (!job.through_all ||
          (!full_thread_depth && requested_thread_depth < available_thread_depth - 1e-7)) {
        trim_thread_tools_at_depth(thread_cutters, thread_axis,
            job.thread_major_diameter * 0.5, job.thread_pitch, requested_thread_depth);
      }
    }
    if (!job.through_all && job.hole_bottom_style == 1) {
      const double half_angle = job.drill_point_angle_deg * kPi / 360.0;
      const double tip_depth =
          (finished_hole_diameter * 0.5) / std::tan(half_angle);
      if (!std::isfinite(tip_depth) || tip_depth <= 0.0) {
        throw std::runtime_error("drill point angle is invalid");
      }
      const gp_Pnt tip_start = support.Translated(
          direction.Multiplied(hole_depth));
      const gp_Ax2 tip_axis(tip_start, gp_Dir(direction));
      // Meet the cylinder at its exact stop disk. Extending the cone upward
      // enlarges its radius beyond the bore and cuts an unintended radial lip.
      BRepPrimAPI_MakeCone drill_point(
          tip_axis, finished_hole_diameter * 0.5, 0.0, tip_depth);
      BRepAlgoAPI_Fuse fuse(cutter, drill_point.Shape(),
                            Message_ProgressRange());
      if (!fuse.IsDone()) {
        throw std::runtime_error("OCCT could not build the drill point cutter");
      }
      cutter = fuse.Shape();
    }
    TopoDS_Shape result;
    if (thread_cutters.empty()) {
      BRepAlgoAPI_Cut cut(found->second, cutter, Message_ProgressRange());
      if (!cut.IsDone() || cut.Shape().IsNull()) {
        throw std::runtime_error("OCCT hole cut failed");
      }
      result = cut.Shape();
    } else if (job.thread_form == 1) {


      BRepAlgoAPI_Cut bore(found->second, cutter, Message_ProgressRange());
      if (!bore.IsDone() || bore.HasErrors() || bore.Shape().IsNull()) {
        throw std::runtime_error("OCCT rounded threaded-hole bore failed");
      }
      result = cut_thread_tools(bore.Shape(), thread_cutters);
    } else {



      result = cut_thread_tools(found->second, thread_cutters);
      BRepAlgoAPI_Cut clean_predrill(
          result, cutter, Message_ProgressRange());
      if (!clean_predrill.IsDone() || clean_predrill.HasErrors() ||
          clean_predrill.Shape().IsNull()) {
        throw std::runtime_error("OCCT threaded-hole predrill cleanup failed");
      }
      result = clean_predrill.Shape();
    }
    if (!thread_cutters.empty()) {
      BRepCheck_Analyzer result_analyzer(result, true, false);
      if (!result_analyzer.IsValid()) {
        throw std::runtime_error("OCCT modeled thread result is invalid");
      }
    }
    found->second = result;
    impl_->imported_display_bodies.erase(job.target_body_ids[0]);
    return;
  }
  if (job.kind == 13) {
    if (job.target_body_ids.size() != 1 || job.face_indices.size() != 1 ||
        job.thread_mode == 0 || job.thread_nominal_diameter <= 0.0 ||
        job.thread_pitch <= 0.0 || job.thread_depth < 0.0) {
      throw std::runtime_error("external thread parameters are malformed");
    }
    if (job.thread_mode == 2 &&
        (job.thread_major_diameter <= job.thread_pitch_diameter ||
         job.thread_pitch_diameter <= job.thread_minor_diameter ||
         job.thread_minor_diameter <= 0.0)) {
      throw std::runtime_error(
          "modeled external thread tolerance limits are malformed");
    }
    auto found = impl_->bodies.find(job.target_body_ids[0]);
    if (found == impl_->bodies.end()) {
      throw std::runtime_error("external thread target body is missing");
    }
    TopTools_IndexedMapOfShape face_map;
    TopExp::MapShapes(found->second, TopAbs_FACE, face_map);
    const std::uint32_t face_index = job.face_indices[0];
    if (face_index >= static_cast<std::uint32_t>(face_map.Extent())) {
      throw std::runtime_error(
          "referenced external-thread cylinder no longer exists");
    }
    const TopoDS_Face face =
        TopoDS::Face(face_map.FindKey(static_cast<int>(face_index) + 1));
    BRepAdaptor_Surface surface(face, true);
    if (surface.GetType() != GeomAbs_Cylinder) {
      throw std::runtime_error(
          "External Thread requires a cylindrical face");
    }
    const gp_Cylinder cylinder = surface.Cylinder();
    const double major_diameter = cylinder.Radius() * 2.0;
    const double diameter_tolerance =
        std::max(0.01, job.thread_nominal_diameter * 0.002);
    if (std::abs(major_diameter - job.thread_nominal_diameter) >
        diameter_tolerance) {
      throw std::runtime_error(
          "selected cylinder does not match the thread major diameter");
    }

    const double first_u = surface.FirstUParameter();
    const double last_u = surface.LastUParameter();
    const double first_v = surface.FirstVParameter();
    const double last_v = surface.LastVParameter();
    if (!std::isfinite(first_u) || !std::isfinite(last_u) ||
        !std::isfinite(first_v) || !std::isfinite(last_v) ||
        std::abs(last_u - first_u) < kTau - 1e-5) {
      throw std::runtime_error(
          "External Thread requires a complete 360-degree cylindrical face");
    }
    const gp_Ax3 cylinder_axes = cylinder.Position();
    const gp_Vec base_axis(cylinder_axes.Direction());
    gp_Pnt sample;
    gp_Vec du;
    gp_Vec dv;
    surface.D1((first_u + last_u) * 0.5, (first_v + last_v) * 0.5,
               sample, du, dv);
    gp_Vec normal = du.Crossed(dv);
    if (face.Orientation() == TopAbs_REVERSED) {
      normal.Reverse();
    }
    gp_Vec radial(cylinder_axes.Location(), sample);
    radial.Subtract(base_axis.Multiplied(radial.Dot(base_axis)));
    if (normal.SquareMagnitude() <= 1e-18 ||
        radial.SquareMagnitude() <= 1e-18 || normal.Dot(radial) <= 0.0) {
      throw std::runtime_error(
          "External Thread requires an outward-facing cylindrical surface");
    }

    const gp_Pnt first_point = surface.Value(first_u, first_v);
    const gp_Pnt last_point = surface.Value(first_u, last_v);
    const double first_offset =
        gp_Vec(cylinder_axes.Location(), first_point).Dot(base_axis);
    const double last_offset =
        gp_Vec(cylinder_axes.Location(), last_point).Dot(base_axis);
    const double lower = std::min(first_offset, last_offset);
    const double upper = std::max(first_offset, last_offset);
    const double available_depth = upper - lower;
    if (!std::isfinite(available_depth) || available_depth <= 1e-7) {
      throw std::runtime_error("external thread cylinder has no axial length");
    }
    const bool full_length = job.thread_depth <= 0.0;
    const double requested_depth =
        full_length ? available_depth : job.thread_depth;
    if (requested_depth > available_depth + 1e-6) {
      throw std::runtime_error(
          "external thread length exceeds the selected cylindrical face");
    }
    gp_Vec direction = base_axis;
    double start_offset = lower;
    if (job.inward) {
      direction.Reverse();
      start_offset = upper;
    }
    const gp_Pnt start = cylinder_axes.Location().Translated(
        base_axis.Multiplied(start_offset));
    const gp_Ax2 thread_axis(
        start, gp_Dir(direction), cylinder_axes.XDirection());
    if (job.thread_mode == 2) {
      TopoDS_Shape result = found->second;
      const double crest_reduction =
          major_diameter - job.thread_major_diameter;
      if (crest_reduction > 1e-7) {



        const double trim_overlap = std::max(1e-4, job.thread_pitch * 1e-4);
        const gp_Pnt trim_start =
            start.Translated(direction.Multiplied(-trim_overlap));
        const gp_Ax2 trim_axis(
            trim_start, gp_Dir(direction), cylinder_axes.XDirection());
        BRepPrimAPI_MakeCylinder outer_trim(
            trim_axis, major_diameter * 0.5 + trim_overlap,
            requested_depth + trim_overlap * 2.0);
        BRepPrimAPI_MakeCylinder inner_keep(
            trim_axis, job.thread_major_diameter * 0.5,
            requested_depth + trim_overlap * 2.0);
        BRepAlgoAPI_Cut sleeve(
            outer_trim.Shape(), inner_keep.Shape(), Message_ProgressRange());
        if (!sleeve.IsDone() || sleeve.HasErrors() ||
            sleeve.Shape().IsNull()) {
          throw std::runtime_error(
              "OCCT could not build the external thread class allowance");
        }
        BRepAlgoAPI_Cut trim(result, sleeve.Shape(), Message_ProgressRange());
        if (!trim.IsDone() || trim.HasErrors() || trim.Shape().IsNull()) {
          throw std::runtime_error(
              "OCCT could not apply the external thread class allowance");
        }
        result = trim.Shape();
      }
      std::vector<TopoDS_Shape> cutters =
          job.thread_form == 1 ? make_rounded_thread_cutters(
              thread_axis, job.thread_major_diameter, job.thread_minor_diameter,
              job.thread_pitch, job.thread_corner_radius, 0.0,
              requested_depth, job.thread_left_hand, false) : make_external_thread_cutters(
              thread_axis, job.thread_major_diameter,
              job.thread_pitch_diameter, job.thread_minor_diameter,
              job.thread_pitch, requested_depth, job.thread_left_hand);
      trim_thread_tools_at_depth(cutters, thread_axis,
          job.thread_major_diameter * 0.5, job.thread_pitch, requested_depth, true);
      GProp_GProps before_thread_properties;
      BRepGProp::VolumeProperties(result, before_thread_properties);
      result = cut_thread_tools(result, cutters);
      BRepCheck_Analyzer result_analyzer(result, true, false);
      if (!result_analyzer.IsValid()) {
        throw std::runtime_error("OCCT modeled external thread result is invalid");
      }
      GProp_GProps result_properties;
      BRepGProp::VolumeProperties(result, result_properties);
      if (!std::isfinite(result_properties.Mass()) ||
          std::abs(result_properties.Mass()) <= 1e-9) {
        throw std::runtime_error(
            "OCCT modeled external thread removed the entire target body");
      }
      const double removed_thread_volume =
          std::abs(before_thread_properties.Mass()) -
          std::abs(result_properties.Mass());
      const double minimum_cut_volume =
          std::max(1e-8, std::abs(before_thread_properties.Mass()) * 1e-8);
      if (!std::isfinite(removed_thread_volume) ||
          removed_thread_volume <= minimum_cut_volume) {
        throw std::runtime_error(
            "OCCT modeled external thread did not remove material");
      }
      found->second = result;
      impl_->imported_display_bodies.erase(job.target_body_ids[0]);
    }
    return;
  }
  if (job.kind == 8) {
    if (job.target_body_ids.size() != 1 || job.face_indices.empty() ||
        !std::isfinite(job.radius) || job.radius <= 0.0) {
      throw std::runtime_error(
          "Shell needs one body, removable faces, and positive thickness");
    }
    auto found = impl_->bodies.find(job.target_body_ids[0]);
    if (found == impl_->bodies.end()) {
      throw std::runtime_error("Shell target body is missing");
    }
    TopTools_IndexedMapOfShape face_map;
    TopExp::MapShapes(found->second, TopAbs_FACE, face_map);
    TopTools_ListOfShape closing_faces;
    for (const std::uint32_t index : job.face_indices) {
      if (index >= static_cast<std::uint32_t>(face_map.Extent())) {
        throw std::runtime_error("referenced Shell face no longer exists");
      }
      closing_faces.Append(face_map.FindKey(index + 1));
    }
    BRepOffsetAPI_MakeThickSolid shell;
    shell.MakeThickSolidByJoin(
        found->second, closing_faces, job.inward ? -job.radius : job.radius,
        1.0e-3, BRepOffset_Skin, false, false, GeomAbs_Arc, true,
        Message_ProgressRange());
    if (!shell.IsDone() || shell.Shape().IsNull()) {
      throw std::runtime_error("OCCT could not build the selected Shell");
    }
    const TopoDS_Shape result = shell.Shape();
    if (!BRepCheck_Analyzer(result, true, false).IsValid()) {
      throw std::runtime_error("OCCT Shell produced invalid geometry; reduce the wall thickness");
    }
    GProp_GProps before_properties, after_properties;
    BRepGProp::VolumeProperties(found->second, before_properties);
    BRepGProp::VolumeProperties(result, after_properties);
    const double before_volume = std::abs(before_properties.Mass());
    const double after_volume = std::abs(after_properties.Mass());
    const double volume_tolerance = std::max(1e-8, before_volume * 1e-8);


    if (!std::isfinite(after_volume) || after_volume <= volume_tolerance ||
        (job.inward && after_volume >= before_volume - volume_tolerance)) {
      throw std::runtime_error("Shell wall thickness leaves no valid hollow body");
    }
    found->second = result;
    impl_->imported_display_bodies.erase(job.target_body_ids[0]);
    return;
  }
  if (job.kind == 9) {
    if (job.target_body_ids.empty() || job.transform_kinds.empty() ||
        job.transform_values.size() != job.transform_kinds.size() * 10 ||
        job.result_body_ids.size() !=
            job.target_body_ids.size() * job.transform_kinds.size()) {
      throw std::runtime_error("body transform buffers are malformed");
    }
    std::size_t output_index = 0;
    for (std::size_t transform_index = 0;
         transform_index < job.transform_kinds.size(); ++transform_index) {
      const std::size_t offset = transform_index * 10;
      gp_Trsf transform;
      if (job.transform_kinds[transform_index] == 0) {
        const gp_Vec normal(job.transform_values[offset + 3],
                            job.transform_values[offset + 4],
                            job.transform_values[offset + 5]);
        if (normal.SquareMagnitude() < 1e-18) {
          throw std::runtime_error("Mirror plane normal is degenerate");
        }
        transform.SetMirror(gp_Ax2(
            gp_Pnt(job.transform_values[offset],
                   job.transform_values[offset + 1],
                   job.transform_values[offset + 2]),
            gp_Dir(normal)));
      } else if (job.transform_kinds[transform_index] == 1) {
        transform.SetTranslation(
            gp_Vec(job.transform_values[offset],
                   job.transform_values[offset + 1],
                   job.transform_values[offset + 2]));
      } else if (job.transform_kinds[transform_index] == 2) {
        const gp_Vec axis(job.transform_values[offset + 3],
                          job.transform_values[offset + 4],
                          job.transform_values[offset + 5]);
        if (axis.SquareMagnitude() < 1e-18) {
          throw std::runtime_error("Circular Pattern axis is degenerate");
        }
        transform.SetRotation(
            gp_Ax1(gp_Pnt(job.transform_values[offset],
                          job.transform_values[offset + 1],
                          job.transform_values[offset + 2]),
                   gp_Dir(axis)),
            job.transform_values[offset + 6]);
      } else if (job.transform_kinds[transform_index] == 3) {
        const double qx = job.transform_values[offset + 3];
        const double qy = job.transform_values[offset + 4];
        const double qz = job.transform_values[offset + 5];
        const double qw = job.transform_values[offset + 6];
        const double magnitude =
            std::sqrt(qx * qx + qy * qy + qz * qz + qw * qw);
        if (!std::isfinite(magnitude) || magnitude <= 1.0e-12) {
          throw std::runtime_error("Move/Copy rotation is degenerate");
        }
        const double x = qx / magnitude;
        const double y = qy / magnitude;
        const double z = qz / magnitude;
        const double w = qw / magnitude;
        const double px = job.transform_values[offset + 7];
        const double py = job.transform_values[offset + 8];
        const double pz = job.transform_values[offset + 9];
        const double tx = job.transform_values[offset];
        const double ty = job.transform_values[offset + 1];
        const double tz = job.transform_values[offset + 2];
        const double r00 = 1.0 - 2.0 * (y * y + z * z);
        const double r01 = 2.0 * (x * y - z * w);
        const double r02 = 2.0 * (x * z + y * w);
        const double r10 = 2.0 * (x * y + z * w);
        const double r11 = 1.0 - 2.0 * (x * x + z * z);
        const double r12 = 2.0 * (y * z - x * w);
        const double r20 = 2.0 * (x * z - y * w);
        const double r21 = 2.0 * (y * z + x * w);
        const double r22 = 1.0 - 2.0 * (x * x + y * y);
        transform.SetValues(
            r00, r01, r02, px + tx - (r00 * px + r01 * py + r02 * pz),
            r10, r11, r12, py + ty - (r10 * px + r11 * py + r12 * pz),
            r20, r21, r22, pz + tz - (r20 * px + r21 * py + r22 * pz));
      } else {
        throw std::runtime_error("unknown body transform kind");
      }
      for (const std::uint64_t source_id : job.target_body_ids) {
        const auto source = impl_->bodies.find(source_id);
        if (source == impl_->bodies.end()) {
          throw std::runtime_error("body transform source is missing");
        }
        BRepBuilderAPI_Transform operation(source->second, transform, true);
        operation.Build(Message_ProgressRange());
        if (!operation.IsDone() || operation.Shape().IsNull()) {
          throw std::runtime_error("OCCT body transform failed");
        }
        impl_->bodies[job.result_body_ids[output_index++]] =
            operation.Shape();
      }
    }
    return;
  }
  if (job.kind == 10) {
    if (job.target_body_ids.size() < 2) {
      throw std::runtime_error("Combine needs a target and at least one tool body");
    }
    const std::uint64_t target_id = job.target_body_ids[0];
    auto target = impl_->bodies.find(target_id);
    if (target == impl_->bodies.end()) {
      throw std::runtime_error("Combine target body is missing");
    }
    TopoDS_Shape result = target->second;
    if (job.operation == 1) {


      TopTools_ListOfShape arguments;
      arguments.Append(result);
      TopTools_ListOfShape tools;
      for (std::size_t index = 1; index < job.target_body_ids.size(); ++index) {
        const auto tool = impl_->bodies.find(job.target_body_ids[index]);
        if (tool == impl_->bodies.end()) {
          throw std::runtime_error("Combine tool body is missing");
        }
        tools.Append(tool->second);
      }
      BRepAlgoAPI_Fuse operation;
      operation.SetArguments(arguments);
      operation.SetTools(tools);
      operation.Build(Message_ProgressRange());
      if (!operation.IsDone() || operation.Shape().IsNull()) {
        throw std::runtime_error("OCCT Combine Join failed");
      }
      operation.SimplifyResult(true, true, 1.0e-7);
      result = operation.Shape();
    }
    for (std::size_t index = 1; index < job.target_body_ids.size(); ++index) {
      if (job.operation == 1) {
        break;
      }
      const auto tool = impl_->bodies.find(job.target_body_ids[index]);
      if (tool == impl_->bodies.end()) {
        throw std::runtime_error("Combine tool body is missing");
      }
      if (job.operation == 2) {
        BRepAlgoAPI_Cut operation(result, tool->second,
                                  Message_ProgressRange());
        if (!operation.IsDone()) {
          throw std::runtime_error("OCCT Combine Cut failed");
        }
        operation.SimplifyResult(true, true, 1.0e-7);
        result = operation.Shape();
      } else if (job.operation == 3) {
        BRepAlgoAPI_Common operation(result, tool->second,
                                     Message_ProgressRange());
        if (!operation.IsDone()) {
          throw std::runtime_error("OCCT Combine Intersect failed");
        }
        operation.SimplifyResult(true, true, 1.0e-7);
        result = operation.Shape();
      } else {
        throw std::runtime_error("unknown Combine operation");
      }
      if (result.IsNull()) {
        throw std::runtime_error("Combine produced a null body");
      }
    }
    impl_->bodies[target_id] = result;
    impl_->imported_display_bodies.erase(target_id);
    if (!job.keep_tools) {
      for (std::size_t index = 1; index < job.target_body_ids.size(); ++index) {
        impl_->bodies.erase(job.target_body_ids[index]);
        impl_->imported_display_bodies.erase(job.target_body_ids[index]);
      }
    }
    return;
  }
  if (job.kind == 11) {
    if (job.target_body_ids.size() != 1 || job.result_body_ids.size() != 2) {
      throw std::runtime_error("Split Body buffers are malformed");
    }
    const auto target = impl_->bodies.find(job.target_body_ids[0]);
    if (target == impl_->bodies.end()) {
      throw std::runtime_error("Split Body target is missing");
    }
    const gp_Vec normal(job.axis_direction_x, job.axis_direction_y,
                        job.axis_direction_z);
    if (normal.SquareMagnitude() < 1e-18) {
      throw std::runtime_error("Split Body plane normal is degenerate");
    }
    const gp_Pnt origin(job.axis_origin_x, job.axis_origin_y,
                        job.axis_origin_z);
    const gp_Vec unit = normal.Normalized();
    BRepBuilderAPI_MakeFace plane(gp_Pln(origin, gp_Dir(unit)));
    if (!plane.IsDone()) {
      throw std::runtime_error("OCCT could not build the splitting plane");
    }
    TopTools_ListOfShape arguments;
    arguments.Append(target->second);
    TopTools_ListOfShape tools;
    tools.Append(plane.Face());
    BRepAlgoAPI_Splitter splitter;
    splitter.SetArguments(arguments);
    splitter.SetTools(tools);
    splitter.SetNonDestructive(true);
    splitter.SetRunParallel(true);
    splitter.Build(Message_ProgressRange());
    if (!splitter.IsDone() || splitter.HasErrors() ||
        splitter.Shape().IsNull()) {
      throw std::runtime_error("OCCT Split Body failed");
    }
    splitter.SimplifyResult(true, true, 1.0e-7);






    struct SplitSolid {
      TopoDS_Shape shape;
      double volume;
      double bounding_volume;
    };
    std::vector<SplitSolid> positive_solids;
    std::vector<SplitSolid> negative_solids;
    for (TopExp_Explorer solids(splitter.Shape(), TopAbs_SOLID); solids.More();
         solids.Next()) {
      const TopoDS_Shape solid = solids.Current();
      GProp_GProps properties;
      BRepGProp::VolumeProperties(solid, properties);
      const double volume = std::abs(properties.Mass());
      const gp_Vec offset(origin, properties.CentreOfMass());
      Bnd_Box solid_bounds;
      BRepBndLib::Add(solid, solid_bounds);
      double sx0 = 0.0;
      double sy0 = 0.0;
      double sz0 = 0.0;
      double sx1 = 0.0;
      double sy1 = 0.0;
      double sz1 = 0.0;
      solid_bounds.Get(sx0, sy0, sz0, sx1, sy1, sz1);
      const double bounding_volume =
          std::max(0.0, sx1 - sx0) * std::max(0.0, sy1 - sy0) *
          std::max(0.0, sz1 - sz0);
      if (!std::isfinite(volume) || volume <= 1e-9) {
        continue;
      }
      const SplitSolid output{solid, volume, bounding_volume};
      if (offset.Dot(unit) >= 0.0) {
        positive_solids.push_back(output);
      } else {
        negative_solids.push_back(output);
      }
    }





    const auto remove_boolean_slivers = [](std::vector<SplitSolid>& solids) {
      if (solids.size() < 2) {
        return;
      }
      const double largest_volume = std::max_element(
          solids.begin(), solids.end(),
          [](const SplitSolid& left, const SplitSolid& right) {
            return left.volume < right.volume;
          })->volume;
      const double relative_limit =
          std::max(1e-8, largest_volume * 1e-5);
      solids.erase(
          std::remove_if(
              solids.begin(), solids.end(),
              [&](const SplitSolid& solid) {
                const double fill_ratio = solid.bounding_volume > 1e-12
                                              ? solid.volume / solid.bounding_volume
                                              : 1.0;
                return solid.volume < relative_limit && fill_ratio < 1e-3;
              }),
          solids.end());
    };
    remove_boolean_slivers(positive_solids);
    remove_boolean_slivers(negative_solids);
    if (positive_solids.empty() || negative_solids.empty()) {
      throw std::runtime_error(
          "Split Body plane does not divide the target into two bodies");
    }
    const auto grouped_shape = [](const std::vector<SplitSolid>& solids) {
      if (solids.size() == 1) {
        return solids.front().shape;
      }
      TopoDS_Compound compound;
      BRep_Builder builder;
      builder.MakeCompound(compound);
      for (const SplitSolid& solid : solids) {
        builder.Add(compound, solid.shape);
      }
      return TopoDS_Shape(compound);
    };
    TopoDS_Shape positive = grouped_shape(positive_solids);
    TopoDS_Shape negative = grouped_shape(negative_solids);
    BRepCheck_Analyzer positive_analyzer(positive, true, false);
    BRepCheck_Analyzer negative_analyzer(negative, true, false);
    if (!positive_analyzer.IsValid() || !negative_analyzer.IsValid()) {





      const auto split_with_halfspace = [&](const gp_Vec& side) {
        const gp_Pnt reference = origin.Translated(side);
        BRepPrimAPI_MakeHalfSpace halfspace(plane.Face(), reference);
        if (!halfspace.IsDone()) {
          throw std::runtime_error(
              "OCCT could not build the Split Body fallback half-space");
        }
        BRepAlgoAPI_Common common(
            target->second, halfspace.Solid(), Message_ProgressRange());
        if (!common.IsDone() || common.HasErrors() ||
            common.Shape().IsNull()) {
          throw std::runtime_error(
              "OCCT Split Body half-space fallback failed");
        }
        common.SimplifyResult(true, true, 1.0e-7);
        TopoDS_Shape result = common.Shape();
        BRepCheck_Analyzer analyzer(result, true, false);
        if (!analyzer.IsValid()) {
          ShapeFix_Shape fixer(result);
          fixer.SetPrecision(1e-7);
          fixer.SetMaxTolerance(1e-5);
          fixer.Perform(Message_ProgressRange());
          result = fixer.Shape();
          BRepLib::SameParameter(result, 1e-6, true);
        }
        return result;
      };
      positive = split_with_halfspace(unit);
      negative = split_with_halfspace(unit.Reversed());
      positive_analyzer = BRepCheck_Analyzer(positive, true, false);
      negative_analyzer = BRepCheck_Analyzer(negative, true, false);
      if (!positive_analyzer.IsValid() || !negative_analyzer.IsValid()) {
        throw std::runtime_error("OCCT Split Body produced invalid geometry");
      }
    }
    impl_->bodies[job.result_body_ids[0]] = positive;
    impl_->bodies[job.result_body_ids[1]] = negative;
    return;
  }
  std::vector<TopoDS_Shape> tools;
  if (job.source_body_id != 0) {
    if (job.kind != 0 || job.source_face_index == UINT32_MAX) {
      throw std::runtime_error("exact face source is only valid for Extrude");
    }
    auto source_body = impl_->bodies.find(job.source_body_id);
    if (source_body == impl_->bodies.end()) {
      throw std::runtime_error("Extrude source body is missing");
    }



    tools.push_back(make_exact_face_tool(
        job, resolve_planar_face_reference(source_body->second, job)));
  } else {
    if (job.profile_offsets.size() < 2 ||
        job.profile_offsets[job.profile_offsets.size() - 1] * 3 !=
            job.points.size()) {
      throw std::runtime_error("profile buffers are malformed");
    }
    if (job.region_offsets.size() < 2 || job.region_offsets.front() != 0 ||
        job.region_offsets.back() + 1 != job.profile_offsets.size()) {
      throw std::runtime_error("profile region buffers are malformed");
    }
    const std::size_t profile_count = job.region_offsets.size() - 1;
    if (job.kind == 3) {
      tools.push_back(make_loft_tool(job));
    } else {
      tools.reserve(profile_count);
      for (std::size_t index = 0; index < profile_count; ++index) {
        tools.push_back(make_tool(job, index));
      }
    }
  }

  if (job.operation == 0) {
    if (job.result_body_ids.size() != tools.size()) {
      throw std::runtime_error("New Body output count does not match profiles");
    }
    for (std::size_t index = 0; index < tools.size(); ++index) {
      impl_->bodies[job.result_body_ids[index]] = tools[index];
    }
    return;
  }
  if (job.operation == 1 && job.target_body_ids.empty()) {
    if (tools.size() < 2 || job.result_body_ids.size() != 1) {
      throw std::runtime_error(
          "Join Profiles needs multiple profiles and one output body");
    }
    impl_->bodies[job.result_body_ids[0]] = fuse_shapes(tools);
    return;
  }
  if (job.target_body_ids.empty()) {
    throw std::runtime_error("boolean solid feature has no target body");
  }
  const TopoDS_Shape tool = fuse_shapes(tools);
  for (const std::uint64_t body_id : job.target_body_ids) {
    auto found = impl_->bodies.find(body_id);
    if (found == impl_->bodies.end()) {
      throw std::runtime_error("boolean target body is missing");
    }
    TopoDS_Shape result;
    if (job.operation == 1) {
      BRepAlgoAPI_Fuse operation(found->second, tool, Message_ProgressRange());
      if (!operation.IsDone()) {
        throw std::runtime_error("OCCT Join failed");
      }
      operation.SimplifyResult(true, true, 1.0e-7);
      result = operation.Shape();
    } else if (job.operation == 2) {
      BRepAlgoAPI_Cut operation(found->second, tool, Message_ProgressRange());
      if (!operation.IsDone()) {
        throw std::runtime_error("OCCT Cut failed");
      }
      operation.SimplifyResult(true, true, 1.0e-7);
      result = operation.Shape();
    } else if (job.operation == 3) {
      BRepAlgoAPI_Common operation(found->second, tool, Message_ProgressRange());
      if (!operation.IsDone()) {
        throw std::runtime_error("OCCT Intersect failed");
      }
      operation.SimplifyResult(true, true, 1.0e-7);
      result = operation.Shape();
    } else {
      throw std::runtime_error("unknown solid operation");
    }
    if (result.IsNull()) {
      throw std::runtime_error("boolean operation produced a null shape");
    }
    found->second = result;
    impl_->imported_display_bodies.erase(body_id);
  }
}

rust::Vec<std::uint64_t> Kernel::body_ids() const {
  rust::Vec<std::uint64_t> result;
  result.reserve(impl_->bodies.size());
  for (const auto& entry : impl_->bodies) {
    result.push_back(entry.first);
  }
  return result;
}

rust::Vec<std::uint64_t> Kernel::planar_face_keys() const {
  rust::Vec<std::uint64_t> result;
  for (const auto& entry : impl_->bodies) {
    TopTools_IndexedMapOfShape faces;
    TopExp::MapShapes(entry.second, TopAbs_FACE, faces);
    for (int index = 1; index <= faces.Extent(); ++index) {
      if (BRepAdaptor_Surface(TopoDS::Face(faces(index)), true).GetType() == GeomAbs_Plane) {
        result.push_back(entry.first);
        result.push_back(static_cast<std::uint64_t>(index - 1));
      }
    }
  }
  return result;
}




static std::string topology_signature(const TopoDS_Shape& shape) {
  std::uint64_t hash = 14695981039346656037ULL;
  const auto mix = [&hash](std::uint64_t value) {
    for (unsigned int byte = 0; byte < 8; ++byte) {
      hash ^= (value >> (byte * 8)) & 0xffU;
      hash *= 1099511628211ULL;
    }
  };
  TopTools_IndexedMapOfShape vertices, edges, faces;
  TopExp::MapShapes(shape, TopAbs_VERTEX, vertices);
  TopExp::MapShapes(shape, TopAbs_EDGE, edges);
  TopExp::MapShapes(shape, TopAbs_FACE, faces);
  mix(1);
  mix(vertices.Extent()); mix(edges.Extent()); mix(faces.Extent());
  for (int index = 1; index <= edges.Extent(); ++index) {
    const TopoDS_Edge edge = TopoDS::Edge(edges.FindKey(index));
    TopoDS_Vertex first, last;
    TopExp::Vertices(edge, first, last, true);
    mix(BRepAdaptor_Curve(edge).GetType());
    mix(edge.Orientation());
    mix(first.IsNull() ? 0 : vertices.FindIndex(first));
    mix(last.IsNull() ? 0 : vertices.FindIndex(last));
  }
  for (int index = 1; index <= faces.Extent(); ++index) {
    const TopoDS_Face face = TopoDS::Face(faces.FindKey(index));
    mix(BRepAdaptor_Surface(face).GetType());
    mix(face.Orientation());
    for (TopExp_Explorer wire(face, TopAbs_WIRE); wire.More(); wire.Next()) {
      mix(0xf1);
      for (BRepTools_WireExplorer edge(TopoDS::Wire(wire.Current()), face);
           edge.More(); edge.Next()) {
        mix(edges.FindIndex(edge.Current()));
        mix(edge.Current().Orientation());
      }
      mix(0xf2);
    }
    mix(0xf3);
  }
  return std::string("connectivity-v1:") + std::to_string(hash);
}







class TangentBoundaryMeshContext : public BRepMesh_Context {
  struct StripPCurve {
    IMeshData::IPCurveHandle curve;
    std::vector<gp_Pnt2d> points;
    std::vector<double> parameters;
    std::vector<int> indices;
  };
  struct StripEdge {
    IMeshData::IEdgePtr edge;
    int status;
    std::vector<gp_Pnt> points;
    std::vector<double> parameters;
    std::vector<StripPCurve> pcurves;
  };
  struct StripFace {
    IMeshData::IFacePtr face;
    int status;
    std::vector<int> wire_statuses;
    Handle(Poly_Triangulation) triangulation;
    std::vector<std::pair<IMeshData::IPCurveHandle, std::vector<int>>> boundary_indices;
    TopAbs_Orientation original_orientation = TopAbs_EXTERNAL;
  };
  struct StripTrial {
    IMeshData::IFacePtr target = nullptr;
    std::array<int,2> target_edges{0,1};
    int connector=-1;
    std::vector<StripEdge> edges;
    std::vector<StripFace> faces;
    double shift = 0.0, angular = 0.0, coverage = 0.0, neighbor_gap = 0.0;
    std::string crossing_certificate;
    bool certified_native_apex = false;
    bool accepted = false;
  };
  struct StripRollbackFailure : std::runtime_error {
    StripRollbackFailure() : std::runtime_error("OCCT could not restore a spherical boundary trial") {}
  };
 public:
  void EnableNativeExportRecovery(bool enabled) { native_export_recovery_=enabled; }
  const std::string& ExportBoundaryRepairStop() const { return export_boundary_stop_; }
  const std::map<int,std::string>& ExportBoundaryRejections() const { return export_boundary_rejections_; }
  const std::string& BoundaryRepairStop() const { return boundary_repair_stop_; }
  std::string StripRepairStop(int face_index) const {
    const auto found = strip_face_rejections_.find(face_index);
    return strip_repair_stop_ + "; " + (found == strip_face_rejections_.end() ?
        "no strip rejection recorded for this face" : found->second.substr(0, 700));
  }

  Standard_Boolean DiscretizeFaces(const Message_ProgressRange& range) override {
    const auto& model = GetModel();
    if (model.IsNull()) return false;
    strip_face_rejections_.clear();
    pole_certificate_work_ = 0;
    strip_continuous_checks_=0;
    export_boundary_work_=0;
    export_boundary_stop_.clear();
    export_boundary_rejections_.clear();
    std::set<IMeshData::IFacePtr> eligible;
    for (int fi = 0; fi < model->FacesNb(); ++fi) {
      const auto& face = model->GetFace(fi);
      if (face->GetSurface()->GetType() != GeomAbs_Sphere ||
          (face->GetStatusMask() & ~IMeshData_Outdated) != 0) continue;
      bool clean_wires = true;
      for (int wi = 0; wi < face->WiresNb(); ++wi)
        clean_wires &= face->GetWire(wi)->GetStatusMask() == 0;
      if (clean_wires) eligible.insert(face.get());
    }
    std::vector<StripTrial> strip_trials;
    TopExp::MapShapes(model->GetShape(), TopAbs_FACE, strip_original_faces_);
    std::set<IMeshData::IFacePtr> strip_faces;
    int strip_attempts = 0;
    std::string strip_rejections;
    try {
    for (int fi = 0; fi < model->FacesNb() && strip_attempts < 16; ++fi) {
      const auto& face = model->GetFace(fi);
      if (face->GetSurface()->GetType() != GeomAbs_Sphere) continue;
      const int original_index = strip_original_faces_.FindIndex(face->GetFace()) - 1;
      if (eligible.count(face.get()) == 0) {
        strip_face_rejections_[original_index] = "prior face/wire status prevents strip trial";
        continue;
      }
      if (face->WiresNb() != 1 || (face->GetWire(0)->EdgesNb() != 2 &&
          !(native_export_recovery_ && face->GetWire(0)->EdgesNb()==3))) {
        strip_face_rejections_[original_index] = "sphere is not a one-wire/two-edge strip";
        continue;
      }
      StripTrial trial;
      if (!prepare_spherical_strip(face, strip_faces, trial, strip_attempts)) {
        strip_face_rejections_[original_index] = strip_stop_.empty() ? "candidate eligibility was not established" : strip_stop_;
        if (!strip_stop_.empty() && strip_rejections.size() < 600)
          strip_rejections += " face " + std::to_string(strip_original_faces_.FindIndex(face->GetFace()) - 1) +
              ": " + strip_stop_.substr(0, 200);
        continue;
      }
      try { strip_trials.push_back(std::move(trial)); }
      catch (...) { restore_spherical_strip(trial); throw; }
      for (const auto& saved : strip_trials.back().faces) strip_faces.insert(saved.face);
    }
    } catch (...) {
      for (const auto& trial : strip_trials) restore_spherical_strip(trial);
      throw;
    }
    Message_ProgressScope stages(range, "Face triangulation and recovery", 49);
    int strip_successes = 0;
    try {
    if (!BRepMesh_Context::DiscretizeFaces(stages.Next())) {
      for (const auto& trial : strip_trials) restore_spherical_strip(trial);
      return false;
    }
    for (auto& trial : strip_trials) {
      if (trial.target->IsSet(IMeshData_Failure) &&
          (trial.target->GetStatusMask() & ~(IMeshData_Outdated | IMeshData_Failure)) == 0)
        retry_spherical_face(trial.target, stages.Next());
      if ((!native_export_recovery_ || (restore_strip_station_nodes(trial) && triangulate_synchronized_strip(trial))) &&
          restore_skipped_strip_nodes(trial,native_export_recovery_) && validate_spherical_strip(trial,native_export_recovery_)) {
        ++strip_successes;
        trial.accepted = true;
      } else {
        strip_face_rejections_[strip_original_faces_.FindIndex(trial.target->GetFace()) - 1] =
            "after meshing: " + strip_stop_ + strip_corner_detail_.substr(0, 550);
        strip_rejections += " face " + std::to_string(strip_original_faces_.FindIndex(trial.target->GetFace()) - 1) +
            " after meshing: " + strip_stop_.substr(0, 200);
        restore_spherical_strip(trial);
        // Restore and remesh only this transaction's adjacent faces before
        // ModelPostProcessor creates their polygon-on-triangulation links.
        Message_ProgressScope recovery(stages.Next(), "Restore spherical neighbors", trial.faces.size());
        BRepMesh_MeshAlgoFactory factory;
        for (const auto& saved : trial.faces) {
          const auto next = recovery.Next();
          if ((saved.status & (IMeshData_Failure | IMeshData_Reused)) != 0) continue;
          const auto algo = factory.GetAlgo(saved.face->GetSurface()->GetType(), GetParameters());
          if (algo.IsNull()) throw std::runtime_error("OCCT could not remesh restored spherical neighbors");
          algo->Perform(saved.face, GetParameters(), next);
        }
      }
    }
    if (native_export_recovery_) {
      recover_export_boundaries();
      recover_export_internal_diagonals();
      recover_export_longest_faces();
      recover_export_chord_boundaries();
      recover_export_longest_faces(false,true);
      recover_export_longest_faces(true);
      recover_export_merged_boundaries();
      diagnose_export_boundary_merges();
    }
    if (!range.More()) {
      for (const auto& trial : strip_trials) restore_spherical_strip(trial);
      return false;
    }
    } catch (const StripRollbackFailure&) { throw; }
      catch (...) {
        for (const auto& trial : strip_trials) restore_spherical_strip(trial);
        throw;
      }
    boundary_repair_stop_ += ", spherical strip attempts/prepared/accepted " +
        std::to_string(strip_attempts) + '/' + std::to_string(strip_trials.size()) + '/' +
        std::to_string(strip_successes);
    strip_repair_stop_ = "strip attempts/prepared/accepted " + std::to_string(strip_attempts) + '/' +
        std::to_string(strip_trials.size()) + '/' + std::to_string(strip_successes);
    for (const auto& trial : strip_trials) {
      if (!trial.accepted) continue;
      std::ostringstream certificate;
      certificate.precision(6);
      certificate << " shift/angular/band/neighbor-gap mm,rad,mm,mm " << trial.shift << '/' << trial.angular << '/' <<
          trial.coverage << '/' << trial.neighbor_gap << trial.crossing_certificate;
      boundary_repair_stop_ += certificate.str();
    }
    if (!strip_rejections.empty()) boundary_repair_stop_ += strip_rejections.substr(0, 800);
    int attempts = 0, successes = 0;
    std::string rejections;
    for (int fi = 0; fi < model->FacesNb() && attempts < 16 && range.More(); ++fi) {
      const auto& face = model->GetFace(fi);
      if (eligible.count(face.get()) == 0 || !face->IsSet(IMeshData_Failure) ||
          (face->GetStatusMask() & ~(IMeshData_Outdated | IMeshData_Failure)) != 0) continue;
      ++attempts;
      if (retry_spherical_face(face, stages.Next())) ++successes;
      else if (attempts - successes <= 2)
        rejections += " face " + std::to_string(fi) + ": " + spherical_retry_stop_.substr(0, 350);
    }
    boundary_repair_stop_ += ", spherical retries/successes " +
        std::to_string(attempts) + '/' + std::to_string(successes);
    if (attempts != successes)
      boundary_repair_stop_ += " rejections" + rejections;
    return true;
  }

  Standard_Boolean HealModel() override {
    const auto& model = GetModel();
    if (model.IsNull()) return false;
    // Prepare compatible circular chords before OCCT amplifies intersecting
    // edges. Then repair any remaining crossings without replacing the
    // boundary joins produced by the standard healer.
    RefinementBudget budget;
    if (!RefineBoundaries(false, budget)) return false;
    if (!BRepMesh_Context::HealModel()) return false;
    return RefineBoundaries(true, budget);
  }

 private:
  Standard_Boolean RefineBoundaries(bool healed, RefinementBudget& budget) {
    budget.context = healed ? "after standard healing" : "before standard healing";
    const auto& model = GetModel();
    std::set<IMeshData::IFacePtr> affected_faces;
    std::set<IMeshData::IFacePtr> intersection_failures;
    constexpr int unrelated_errors = IMeshData_OpenWire | IMeshData_TooFewPoints |
        IMeshData_UnorientedWire | IMeshData_UserBreak;
    for (int fi = 0; fi < model->FacesNb(); ++fi) {
      const auto& face = model->GetFace(fi);
      if (face->IsSet(IMeshData_SelfIntersectingWire) &&
          (face->GetStatusMask() & unrelated_errors) == 0) {
        bool other_wire_failure = false;
        for (int wi = 0; wi < face->WiresNb(); ++wi) {
          const auto& wire = face->GetWire(wi);
          other_wire_failure |= (wire->GetStatusMask() & unrelated_errors) != 0 ||
              (wire->IsSet(IMeshData_Failure) &&
               !wire->IsSet(IMeshData_SelfIntersectingWire));
        }
        if (!other_wire_failure) intersection_failures.insert(face.get());
      }
    }
    const auto rebuild_pcurves = [&](IMeshData::IEdgePtr edge) {
      // Tessellate2d regenerates each owner pcurve from the full edge samples.
      // Reserve its aggregate work before clearing or allocating any pcurve.
      budget.sample(static_cast<std::size_t>(edge->GetCurve()->ParametersNb()),
                    static_cast<std::size_t>(edge->PCurvesNb()));
      // The retained endpoints and affected-face tracking also grow here.
      budget.sample(static_cast<std::size_t>(edge->PCurvesNb()), 2);
      struct Endpoints {
        IMeshData::IPCurveHandle pcurve;
        gp_Pnt2d first;
        gp_Pnt2d last;
        double first_parameter;
        double last_parameter;
        TopAbs_Orientation orientation;
      };
      std::vector<Endpoints> endpoints;
      edge->SetStatus(IMeshData_Outdated);
      for (int pi = 0; pi < edge->PCurvesNb(); ++pi) {
        const auto& pcurve = edge->GetPCurve(pi);
        const int count = pcurve->ParametersNb();
        if (healed && count >= 2)
          endpoints.push_back({pcurve, pcurve->GetPoint(0), pcurve->GetPoint(count - 1),
              pcurve->GetParameter(0), pcurve->GetParameter(count - 1),
              pcurve->GetOrientation()});
        pcurve->Clear(false);
        const auto& affected = pcurve->GetFace();
        affected->SetStatus(IMeshData_Outdated);
        affected_faces.insert(affected);
        if (!healed && affected->IsSet(IMeshData_SelfIntersectingWire) &&
            (affected->GetStatusMask() & unrelated_errors) == 0) {
          affected->UnsetStatus(IMeshData_SelfIntersectingWire);
          affected->UnsetStatus(IMeshData_Failure);
        }
      }
      BRepMesh_EdgeDiscret::Tessellate2d(edge, true);
      // Preserve the healer's connected endpoints only when their parameter
      // and orientation correspondence survived regeneration unchanged.
      for (const auto& saved : endpoints) {
        const int count = saved.pcurve->ParametersNb();
        if (count < 2 || saved.pcurve->GetOrientation() != saved.orientation ||
            saved.pcurve->GetParameter(0) != saved.first_parameter ||
            saved.pcurve->GetParameter(count - 1) != saved.last_parameter)
          continue;
        saved.pcurve->GetPoint(0) = saved.first;
        saved.pcurve->GetPoint(count - 1) = saved.last;
      }
    };
    int junction_attempts = 0, junction_repairs = 0;
    // Collapse only a crossing's contiguous samples inside the shared CAD
    // vertex's tolerance neighborhood. Every adjacent face must still pass.
    const auto repair_junction = [&](const IMeshData::IFaceHandle& face,
                                    const Handle(IMeshData::MapOfIEdgePtr)& crossings,
                                    int face_index) {
      try {
        if (crossings.IsNull() || junction_attempts >= 16 ||
            face->GetSurface()->IsUPeriodic() || face->GetSurface()->IsVPeriodic() ||
            (face->GetStatusMask() & unrelated_errors) != 0 ||
            (face->IsSet(IMeshData_Failure) && intersection_failures.count(face.get()) == 0))
          return false;
        budget.context = std::string(healed ? "after" : "before") +
            " standard healing, junction repair, face " + std::to_string(face_index);
        for (int wi = 0; wi < face->WiresNb(); ++wi) {
          budget.compare(1);
          const auto& wire = face->GetWire(wi);
          if ((wire->GetStatusMask() & unrelated_errors) != 0 ||
              (wire->IsSet(IMeshData_Failure) && !wire->IsSet(IMeshData_SelfIntersectingWire)))
            return false;
        }
        const auto finite_point = [](const gp_Pnt& point) {
          return std::isfinite(point.X()) && std::isfinite(point.Y()) && std::isfinite(point.Z());
        };
        const auto finite_uv = [](const gp_Pnt2d& point) {
          return std::isfinite(point.X()) && std::isfinite(point.Y());
        };
        const double deflection = GetParameters().Deflection;
        if (!std::isfinite(deflection) || deflection <= 0.0) return false;
        for (int wi = 0; wi < face->WiresNb(); ++wi) {
          const auto& wire = face->GetWire(wi);
          for (int ei = 0; ei < wire->EdgesNb(); ++ei) {
            budget.context = std::string(healed ? "after" : "before") +
                " standard healing, junction repair, face " + std::to_string(face_index) +
                ", wire " + std::to_string(wi) + ", edge " + std::to_string(ei);
            budget.compare(1);
            const int ni = (ei + 1) % wire->EdgesNb();
            auto a = wire->GetEdge(ei), b = wire->GetEdge(ni);
            if (a == b || !crossings->Contains(a) || !crossings->Contains(b) ||
                !a->GetSameParam() || !b->GetSameParam() ||
                !a->GetSameRange() || !b->GetSameRange() ||
                a->PCurvesNb() > 16 || b->PCurvesNb() > 16) continue;
            TopoDS_Vertex vertex;
            if (!TopExp::CommonVertex(a->GetEdge(), b->GetEdge(), vertex)) continue;
            const gp_Pnt center = BRep_Tool::Pnt(vertex);
            const double vertex_tolerance = BRep_Tool::Tolerance(vertex);
            const double a_tolerance = BRep_Tool::Tolerance(a->GetEdge());
            const double b_tolerance = BRep_Tool::Tolerance(b->GetEdge());
            if (!finite_point(center) || !std::isfinite(vertex_tolerance) || vertex_tolerance <= 0.0 ||
                !std::isfinite(a_tolerance) || a_tolerance < 0.0 ||
                !std::isfinite(b_tolerance) || b_tolerance < 0.0) continue;
            const auto& ap = a->GetPCurve(face.get(), wire->GetEdgeOrientation(ei));
            const auto& bp = b->GetPCurve(face.get(), wire->GetEdgeOrientation(ni));
            if (ap.IsNull() || bp.IsNull()) continue;
            const auto endpoint = [&](IMeshData::IEdgePtr edge) {
              const auto& curve = edge->GetCurve();
              const int count = curve->ParametersNb();
              if (count < 2 || count > 4096) return -1;
              TopoDS_Vertex first_vertex, last_vertex;
              TopExp::Vertices(edge->GetEdge(), first_vertex, last_vertex);
              const bool first = !first_vertex.IsNull() && first_vertex.IsSame(vertex) &&
                  curve->GetPoint(0).SquareDistance(center) <= Precision::SquareConfusion();
              const bool last = !last_vertex.IsNull() && last_vertex.IsSame(vertex) &&
                  curve->GetPoint(count - 1).SquareDistance(center) <= Precision::SquareConfusion();
              return first == last ? -1 : first ? 0 : count - 1;
            };
            const int ae = endpoint(a), be = endpoint(b);
            if (ae < 0 || be < 0 ||
                ap->ParametersNb() != a->GetCurve()->ParametersNb() ||
                bp->ParametersNb() != b->GetCurve()->ParametersNb()) continue;
            BRepAdaptor_Curve ac(a->GetEdge()), bc(b->GetEdge());
            BRepAdaptor_Curve af(TopoDS::Edge(a->GetEdge().Oriented(ap->GetOrientation())), face->GetFace());
            BRepAdaptor_Curve bf(TopoDS::Edge(b->GetEdge().Oriented(bp->GetOrientation())), face->GetFace());
            budget.compare(static_cast<std::size_t>(ap->ParametersNb() - 1),
                           static_cast<std::size_t>(bp->ParametersNb() - 1));
            for (int ai = 1; ai < ap->ParametersNb(); ++ai) {
              for (int bi = 1; bi < bp->ParametersNb(); ++bi) {
                const auto& p = ap->GetPoint(ai - 1); const auto& q = ap->GetPoint(ai);
                const auto& r = bp->GetPoint(bi - 1); const auto& s = bp->GetPoint(bi);
                if (!finite_uv(p) || !finite_uv(q) || !finite_uv(r) || !finite_uv(s)) continue;
                if (std::max(p.X(), q.X()) < std::min(r.X(), s.X()) ||
                    std::max(r.X(), s.X()) < std::min(p.X(), q.X()) ||
                    std::max(p.Y(), q.Y()) < std::min(r.Y(), s.Y()) ||
                    std::max(r.Y(), s.Y()) < std::min(p.Y(), q.Y())) continue;
                gp_Pnt2d uv;
                if (BRepMesh_GeomTool::IntSegSeg(p.Coord(), q.Coord(), r.Coord(), s.Coord(),
                        false, false, uv) != BRepMesh_GeomTool::Cross) continue;
                if (!finite_uv(uv)) continue;
                const gp_XY av = q.Coord() - p.Coord(), bv = s.Coord() - r.Coord();
                if (!std::isfinite(av.SquareModulus()) || !std::isfinite(bv.SquareModulus()) ||
                    av.SquareModulus() <= 0.0 || bv.SquareModulus() <= 0.0) continue;
                const double at = ap->GetParameter(ai - 1) +
                    (uv.Coord() - p.Coord()).Dot(av) / av.SquareModulus() *
                        (ap->GetParameter(ai) - ap->GetParameter(ai - 1));
                const double bt = bp->GetParameter(bi - 1) +
                    (uv.Coord() - r.Coord()).Dot(bv) / bv.SquareModulus() *
                        (bp->GetParameter(bi) - bp->GetParameter(bi - 1));
                if (!std::isfinite(at) || !std::isfinite(bt)) continue;
                const auto auv = af.CurveOnSurface().GetCurve()->Value(at);
                const auto buv = bf.CurveOnSurface().GetCurve()->Value(bt);
                if (!finite_uv(auv) || !finite_uv(buv)) continue;
                const double aet = ap->GetParameter(ae), bet = bp->GetParameter(be);
                if (!std::isfinite(aet) || !std::isfinite(bet)) continue;
                const auto aeuv = af.CurveOnSurface().GetCurve()->Value(aet);
                const auto beuv = bf.CurveOnSurface().GetCurve()->Value(bet);
                if (!finite_uv(aeuv) || !finite_uv(beuv)) continue;
                const double ad = face->GetSurface()->Value(auv.X(), auv.Y()).Distance(ac.Value(at));
                const double bd = face->GetSurface()->Value(buv.X(), buv.Y()).Distance(bc.Value(bt));
                const double aed = face->GetSurface()->Value(aeuv.X(), aeuv.Y()).Distance(ac.Value(aet));
                const double bed = face->GetSurface()->Value(beuv.X(), beuv.Y()).Distance(bc.Value(bet));
                const double crossing_distance = face->GetSurface()->Value(uv.X(), uv.Y()).Distance(center);
                // Include the measured source representation error at this
                // junction, always bounded by its edge's existing tolerance.
                if (!std::isfinite(ad) || !std::isfinite(bd) ||
                    !std::isfinite(aed) || !std::isfinite(bed) ||
                    !std::isfinite(crossing_distance) || ad > a_tolerance || bd > b_tolerance ||
                    aed > a_tolerance || bed > b_tolerance ||
                    crossing_distance >
                        vertex_tolerance + std::max({ad, bd, aed, bed, Precision::Confusion()})) continue;
                const auto near_vertex_path = [&](IMeshData::IEdgePtr edge, int end, int segment) {
                  const auto& curve = edge->GetCurve();
                  const int first = end == 0 ? 1 : segment;
                  const int last = end == 0 ? segment - 1 : curve->ParametersNb() - 2;
                  const double limit = vertex_tolerance + BRep_Tool::Tolerance(edge->GetEdge());
                  const gp_Pnt retained = curve->GetPoint(end == 0 ? segment : segment - 1);
                  if (!finite_point(retained)) return false;
                  const gp_Vec chord(center, retained);
                  const double length_squared = chord.SquareMagnitude();
                  if (!std::isfinite(length_squared)) return false;
                  if (last >= first)
                    budget.compare(static_cast<std::size_t>(last - first + 1));
                  for (int index = first; index <= last; ++index) {
                    const auto& point = curve->GetPoint(index);
                    if (!finite_point(point) || !std::isfinite(curve->GetParameter(index))) return false;
                    const double distance = point.Distance(center);
                    if (!std::isfinite(distance) || distance > limit) return false;
                    const double fraction = length_squared > 0.0
                        ? std::clamp(gp_Vec(center, point).Dot(chord) / length_squared, 0.0, 1.0) : 0.0;
                    const double error = point.Distance(center.Translated(chord.Multiplied(fraction)));
                    if (!std::isfinite(error) || error > deflection) return false;
                  }
                  return true;
                };
                if (!near_vertex_path(a, ae, ai) || !near_vertex_path(b, be, bi)) continue;
                if (junction_attempts >= 16) return false;
                struct PCurveState {
                  IMeshData::IPCurveHandle curve;
                  std::vector<gp_Pnt2d> points;
                  std::vector<double> parameters;
                  std::vector<int> indices;
                };
                struct EdgeState {
                  IMeshData::IEdgePtr edge;
                  int status;
                  std::vector<gp_Pnt> points;
                  std::vector<double> parameters;
                  std::vector<PCurveState> pcurves;
                };
                struct FaceState { IMeshData::IFacePtr face; int status; std::vector<int> wires; };
                budget.sample(2); // saved EdgeState records
                std::vector<EdgeState> saved_edges;
                std::map<IMeshData::IFacePtr, FaceState> saved_faces;
                for (auto edge : {a, b}) {
                  EdgeState saved{edge, edge->GetStatusMask(), {}, {}, {}};
                  const auto& curve = edge->GetCurve();
                  // Two snapshot arrays plus a possible native restoration.
                  budget.sample(static_cast<std::size_t>(curve->ParametersNb()), 3);
                  for (int index = 0; index < curve->ParametersNb(); ++index) {
                    saved.points.push_back(curve->GetPoint(index));
                    saved.parameters.push_back(curve->GetParameter(index));
                  }
                  budget.sample(static_cast<std::size_t>(edge->PCurvesNb()));
                  for (int pi = 0; pi < edge->PCurvesNb(); ++pi) {
                    const auto& pc = edge->GetPCurve(pi);
                    // Three snapshot arrays plus a possible native restoration.
                    budget.sample(static_cast<std::size_t>(pc->ParametersNb()), 4);
                    PCurveState saved_pc{pc, {}, {}, {}};
                    for (int index = 0; index < pc->ParametersNb(); ++index) {
                      saved_pc.points.push_back(pc->GetPoint(index));
                      saved_pc.parameters.push_back(pc->GetParameter(index));
                      saved_pc.indices.push_back(pc->GetIndex(index));
                    }
                    saved.pcurves.push_back(std::move(saved_pc));
                    auto* adjacent = pc->GetFace();
                    if (saved_faces.count(adjacent) == 0) {
                      budget.sample(); // saved FaceState map entry
                      budget.sample(static_cast<std::size_t>(adjacent->WiresNb()));
                      FaceState state{adjacent, adjacent->GetStatusMask(), {}};
                      for (int index = 0; index < adjacent->WiresNb(); ++index)
                        state.wires.push_back(adjacent->GetWire(index)->GetStatusMask());
                      saved_faces.emplace(adjacent, std::move(state));
                    }
                  }
                  saved_edges.push_back(std::move(saved));
                }
                // Include the snapshot and its restoration before copying.
                budget.sample(affected_faces.size(), 2);
                const auto old_affected_faces = affected_faces;
                const auto restore_status = [](auto* item, int status) {
                  item->UnsetStatus(static_cast<IMeshData_Status>(item->GetStatusMask()));
                  item->SetStatus(static_cast<IMeshData_Status>(status));
                };
                const auto rollback = [&]() {
                  for (const auto& saved : saved_edges) {
                    const auto& curve = saved.edge->GetCurve();
                    curve->Clear(false);
                    for (std::size_t index = 0; index < saved.points.size(); ++index)
                      curve->AddPoint(saved.points[index], saved.parameters[index]);
                    for (const auto& pc : saved.pcurves) {
                      pc.curve->Clear(false);
                      for (std::size_t index = 0; index < pc.points.size(); ++index) {
                        pc.curve->AddPoint(pc.points[index], pc.parameters[index]);
                        pc.curve->GetIndex(static_cast<int>(index)) = pc.indices[index];
                      }
                    }
                    restore_status(saved.edge, saved.status);
                  }
                  for (const auto& entry : saved_faces) {
                    restore_status(entry.first, entry.second.status);
                    for (std::size_t index = 0; index < entry.second.wires.size(); ++index)
                      restore_status(entry.first->GetWire(static_cast<int>(index)).get(), entry.second.wires[index]);
                  }
                  affected_faces = old_affected_faces;
                };
                ++junction_attempts;
                try {
                  for (int which = 0; which < 2; ++which) {
                    const auto& saved = saved_edges[which];
                    const int end = which == 0 ? ae : be, segment = which == 0 ? ai : bi;
                    const auto& curve = saved.edge->GetCurve();
                    curve->Clear(false);
                    for (int index = 0; index < static_cast<int>(saved.points.size()); ++index) {
                      if (index > 0 && index < static_cast<int>(saved.points.size()) - 1 &&
                          (end == 0 ? index < segment : index >= segment)) continue;
                      curve->AddPoint(saved.points[index], saved.parameters[index]);
                    }
                    rebuild_pcurves(saved.edge);
                  }
                  ap->GetPoint(ae == 0 ? 0 : ap->ParametersNb() - 1) = uv;
                  bp->GetPoint(be == 0 ? 0 : bp->ParametersNb() - 1) = uv;
                  bool valid = true;
                  for (const auto& entry : saved_faces) {
                    BRepMesh_FaceChecker after(IMeshData::IFaceHandle(entry.first), GetParameters());
                    if (!after.Perform()) { valid = false; break; }
                  }
                  if (valid) { ++junction_repairs; return true; }
                } catch (const RefinementBudgetExceeded& error) {
                  try {
                    rollback();
                  } catch (...) {
                    throw RefinementBudgetExceeded(std::string(error.what()) +
                        " Junction rollback also failed; meshing was aborted.");
                  }
                  throw;
                } catch (const Standard_Failure&) {
                  rollback();
                  continue;
                } catch (const std::exception&) {
                  rollback();
                  continue;
                }
                rollback();
              }
            }
          }
        }
        return false;
      } catch (const RefinementBudgetExceeded&) {
        throw;
      } catch (const Standard_Failure&) {
        return false;
      } catch (const std::exception&) {
        return false;
      }
    };
    const auto check_repaired_faces = [&]() {
      constexpr int max_boundary_passes = 8;
      constexpr int max_edge_points = 4096;
      constexpr std::size_t max_added_points = 65536;
      std::size_t added_points = 0;
      for (int pass = 0; pass <= max_boundary_passes; ++pass) {
        std::set<IMeshData::IEdgePtr> intersecting_edges;
        for (int fi = 0; fi < model->FacesNb(); ++fi) {
          const auto& face = model->GetFace(fi);
          if (affected_faces.count(face.get()) == 0 &&
              !face->IsSet(IMeshData_SelfIntersectingWire)) continue;
          BRepMesh_FaceChecker checker(face, GetParameters());
          bool valid = checker.Perform();
          if (!valid && repair_junction(face, checker.GetIntersectingEdges(), fi))
            valid = checker.Perform();
          if (!valid) {
            if (!face->IsSet(IMeshData_Failure) &&
                (face->GetStatusMask() & unrelated_errors) == 0)
              intersection_failures.insert(face.get());
            face->SetStatus(IMeshData_SelfIntersectingWire);
            face->SetStatus(IMeshData_Failure);
            const auto& edges = checker.GetIntersectingEdges();
            if (!edges.IsNull()) {
              for (IMeshData::MapOfIEdgePtr::Iterator edge(*edges);
                   edge.More(); edge.Next())
                intersecting_edges.insert(edge.Value());
            }
            continue;
          }
          face->UnsetStatus(IMeshData_SelfIntersectingWire);
          if (intersection_failures.count(face.get()) != 0)
            face->UnsetStatus(IMeshData_Failure);
          for (int wi = 0; wi < face->WiresNb(); ++wi) {
            const auto& wire = face->GetWire(wi);
            if (!wire->IsSet(IMeshData_SelfIntersectingWire)) continue;
            wire->UnsetStatus(IMeshData_SelfIntersectingWire);
            if ((wire->GetStatusMask() & unrelated_errors) == 0)
              wire->UnsetStatus(IMeshData_Failure);
          }
        }
        if (intersecting_edges.empty() || pass == max_boundary_passes) {
          boundary_repair_stop_ = intersecting_edges.empty()
              ? "no reported boundary intersections"
              : "8 pass limit";
          boundary_repair_stop_ += ", added points " + std::to_string(added_points);
          boundary_repair_stop_ += ", junction attempts/repairs " +
              std::to_string(junction_attempts) + '/' + std::to_string(junction_repairs);
          break;
        }
        bool inserted = false;
        bool parameter_mismatch = false, edge_limit = false, point_limit = false;
        for (auto edge : intersecting_edges) {
          if (!edge->GetSameParam() || !edge->GetSameRange()) {
            parameter_mismatch = true;
            continue;
          }
          const auto& points = edge->GetCurve();
          const int count = points->ParametersNb();
          if (count < 2) continue;
          if (count > (max_edge_points + 1) / 2) {
            edge_limit = true;
            continue;
          }
          if (static_cast<std::size_t>(count - 1) > max_added_points - added_points) {
            point_limit = true;
            continue;
          }
          BRepAdaptor_Curve curve(edge->GetEdge());
          bool edge_inserted = false;
          // Refine exact curve samples without replacing the samples already
          // needed by adjacent faces or the circular-boundary repair.
          for (int index = count - 1; index > 0; --index) {
            const double first = points->GetParameter(index - 1);
            const double last = points->GetParameter(index);
            const double middle = first + (last - first) * 0.5;
            if (!std::isfinite(middle) || middle == first || middle == last)
              continue;
            budget.insert(static_cast<std::size_t>(points->ParametersNb()));
            points->InsertPoint(index, curve.Value(middle), middle);
            ++added_points;
            inserted = true;
            edge_inserted = true;
          }
          if (!edge_inserted) continue;
          rebuild_pcurves(edge);
        }
        if (!inserted) {
          boundary_repair_stop_ = edge_limit ? "4096 edge point limit"
              : point_limit ? "65536 added point limit"
              : parameter_mismatch ? "nonmatching edge parameters"
              : "no representable midpoint";
          boundary_repair_stop_ += ", added points " + std::to_string(added_points);
          boundary_repair_stop_ += ", junction attempts/repairs " +
              std::to_string(junction_attempts) + '/' + std::to_string(junction_repairs);
          break;
        }
      }
      return Standard_True;
    };

    constexpr int max_refinement_passes = 16;
    for (int pass = 0; pass <= max_refinement_passes; ++pass) {
      std::map<IMeshData::IEdgePtr, std::vector<double>> additions;
      bool crossing = false;
      std::string crossing_detail;
      for (int fi = 0; fi < model->FacesNb(); ++fi) {
        const auto& face = model->GetFace(fi);
        // After standard healing, leave accepted planes alone: further
        // refinement can introduce tiny slivers on otherwise valid thread ends.
        if (face->GetSurface()->GetType() != GeomAbs_Plane ||
            (healed && intersection_failures.count(face.get()) == 0)) continue;
        for (int wi = 0; wi < face->WiresNb(); ++wi) {
          const auto& wire = face->GetWire(wi);
          for (int ei = 0; ei < wire->EdgesNb(); ++ei) {
            const int ni = (ei + 1) % wire->EdgesNb();
            auto a = wire->GetEdge(ei);
            auto b = wire->GetEdge(ni);
            if (a == b || !a->GetSameParam() || !b->GetSameParam() ||
                !a->GetSameRange() || !b->GetSameRange()) continue;
            BRepAdaptor_Curve ac(a->GetEdge()), bc(b->GetEdge());
            const bool a_circle = ac.GetType() == GeomAbs_Circle;
            if (a_circle == (bc.GetType() == GeomAbs_Circle)) continue;
            const auto& ap = a->GetPCurve(face.get(), wire->GetEdgeOrientation(ei));
            const auto& bp = b->GetPCurve(face.get(), wire->GetEdgeOrientation(ni));
            budget.context = std::string(healed ? "after" : "before") +
                " standard healing, face " + std::to_string(fi) + ", wire " +
                std::to_string(wi) + ", edges " + std::to_string(ei) + '/' +
                std::to_string(ni) + ", pass " + std::to_string(pass);
            budget.compare(static_cast<std::size_t>(std::max(0, ap->ParametersNb() - 1)),
                           static_cast<std::size_t>(std::max(0, bp->ParametersNb() - 1)));
            TopoDS_Vertex shared_vertex;
            const bool has_shared_vertex =
                TopExp::CommonVertex(a->GetEdge(), b->GetEdge(), shared_vertex);
            const double junction_tolerance = has_shared_vertex
                ? std::max({Precision::Confusion(),
                            BRep_Tool::Tolerance(shared_vertex),
                            BRep_Tool::Tolerance(a->GetEdge()),
                            BRep_Tool::Tolerance(b->GetEdge())})
                : Precision::Confusion();
            auto cross = [](const gp_Pnt2d& p, const gp_Pnt2d& q, const gp_Pnt2d& r) {
              return (q.X()-p.X())*(r.Y()-p.Y()) - (q.Y()-p.Y())*(r.X()-p.X());
            };
            for (int ai = 1; ai < ap->ParametersNb(); ++ai) {
              for (int bi = 1; bi < bp->ParametersNb(); ++bi) {
                const auto& p = ap->GetPoint(ai-1); const auto& q = ap->GetPoint(ai);
                const auto& r = bp->GetPoint(bi-1); const auto& s = bp->GetPoint(bi);



                if (std::min({p.SquareDistance(r), p.SquareDistance(s),
                              q.SquareDistance(r), q.SquareDistance(s)}) <=
                    Precision::SquareConfusion()) continue;
                if (cross(p,q,r)*cross(p,q,s) >= -1e-18 ||
                    cross(r,s,p)*cross(r,s,q) >= -1e-18) continue;
                const double denominator =
                    (q.X()-p.X())*(s.Y()-r.Y()) -
                    (q.Y()-p.Y())*(s.X()-r.X());
                const double fraction =
                    ((r.X()-p.X())*(s.Y()-r.Y()) -
                     (r.Y()-p.Y())*(s.X()-r.X())) / denominator;
                const gp_Pnt2d intersection(
                    p.X() + fraction*(q.X()-p.X()),
                    p.Y() + fraction*(q.Y()-p.Y()));
                const double junction_distance = has_shared_vertex
                    ? face->GetSurface()->Value(intersection.X(), intersection.Y())
                          .Distance(BRep_Tool::Pnt(shared_vertex))
                    : std::numeric_limits<double>::infinity();
                // STEP vertices can tolerate a junction wider than Confusion.
                // Refining a crossing inside that junction cannot repair the
                // exact curves and can exhaust every pass on valid geometry.
                if (has_shared_vertex && junction_distance <= junction_tolerance)
                    continue;
                if (crossing_detail.empty()) {
                  const auto curve_name = [](GeomAbs_CurveType type) {
                    switch (type) {
                      case GeomAbs_Line: return "line";
                      case GeomAbs_Circle: return "circle";
                      case GeomAbs_Ellipse: return "ellipse";
                      case GeomAbs_Hyperbola: return "hyperbola";
                      case GeomAbs_Parabola: return "parabola";
                      case GeomAbs_BezierCurve: return "Bezier";
                      case GeomAbs_BSplineCurve: return "B-spline";
                      case GeomAbs_OffsetCurve: return "offset";
                      default: return "other";
                    }
                  };
                  std::ostringstream detail;
                  detail.precision(17);
                  detail << " (face " << fi << ", wire " << wi
                         << ", edges " << ei << '/' << ni
                         << ", curves " << curve_name(ac.GetType()) << '/'
                         << curve_name(bc.GetType())
                         << ", parameters " << ap->GetParameter(ai-1) << ':'
                         << ap->GetParameter(ai) << '/' << bp->GetParameter(bi-1)
                         << ':' << bp->GetParameter(bi)
                         << ", junction distance mm " << junction_distance
                         << ", junction tolerance mm " << junction_tolerance
                         << ", pass " << pass << ')';
                  crossing_detail = detail.str();
                }
                crossing = true;
                auto circular = a_circle ? a : b;
                auto other = a_circle ? b : a;
                const int oi = a_circle ? bi : ai;
                BRepAdaptor_Curve curve(circular->GetEdge());
                BRepAdaptor_Curve other_curve(other->GetEdge());
                const auto& other_pcurve = a_circle ? bp : ap;
                const double first = curve.FirstParameter(), last = curve.LastParameter();
                const double middle = (other_pcurve->GetParameter(oi-1) + other_pcurve->GetParameter(oi)) * 0.5;
                budget.sample();
                additions[other].push_back(middle);
                for (double sample : {other_pcurve->GetParameter(oi-1), middle, other_pcurve->GetParameter(oi)}) {


                  const gp_Pnt point = other_curve.Value(sample);
                  double parameter = ElCLib::Parameter(curve.Circle(), point);
                  parameter += kTau * std::ceil((first - parameter) / kTau);
                  if (parameter > first+1e-10 && parameter < last-1e-10) {
                    budget.sample();
                    additions[circular].push_back(parameter);
                  }
                }
              }
            }
          }
        }
      }



      if (!crossing) return healed ? check_repaired_faces() : Standard_True;
      if (additions.empty() || pass == max_refinement_passes) {
        throw std::runtime_error("OCCT could not discretize tangential face boundaries without crossing chords" + crossing_detail);
      }
      bool inserted = false;
      for (auto& entry : additions) {
        auto edge = entry.first;
        auto& parameters = entry.second;
        std::sort(parameters.begin(), parameters.end());
        BRepAdaptor_Curve curve(edge->GetEdge());
        const auto& points = edge->GetCurve();
        const bool ascending = points->GetParameter(0) <
            points->GetParameter(points->ParametersNb()-1);
        bool edge_inserted = false;
        for (double parameter : parameters) {
          budget.compare(static_cast<std::size_t>(points->ParametersNb()));
          int index = 0;
          while (index < points->ParametersNb() &&
                 (ascending ? points->GetParameter(index) < parameter :
                              points->GetParameter(index) > parameter)) ++index;
          if ((index < points->ParametersNb() && std::abs(points->GetParameter(index)-parameter) < 1e-10) ||
              (index > 0 && std::abs(points->GetParameter(index-1)-parameter) < 1e-10)) continue;
          budget.insert(static_cast<std::size_t>(points->ParametersNb()));
          points->InsertPoint(index, curve.Value(parameter), parameter);
          inserted = true;
          edge_inserted = true;
        }
        if (!edge_inserted) continue;
        rebuild_pcurves(edge);
      }
      if (!inserted) {
        throw std::runtime_error("OCCT could not refine crossing tangential face boundaries" + crossing_detail);
      }
    }
    return true;
  }

 private:
  static bool strip_finite(const gp_Pnt& p) {
    return std::isfinite(p.X()) && std::isfinite(p.Y()) && std::isfinite(p.Z());
  }
  static bool strip_finite(const gp_Pnt2d& p) {
    return std::isfinite(p.X()) && std::isfinite(p.Y());
  }
  static void restore_spherical_strip(const StripTrial& trial) {
    try {
      for (const auto& saved : trial.edges) {
        const auto& curve = saved.edge->GetCurve();
        curve->Clear(false);
        for (std::size_t i = 0; i < saved.points.size(); ++i)
          curve->AddPoint(saved.points[i], saved.parameters[i]);
        saved.edge->UnsetStatus(static_cast<IMeshData_Status>(saved.edge->GetStatusMask()));
        saved.edge->SetStatus(static_cast<IMeshData_Status>(saved.status));
        for (const auto& pc : saved.pcurves) {
          pc.curve->Clear(false);
          for (std::size_t i = 0; i < pc.points.size(); ++i) {
            pc.curve->AddPoint(pc.points[i], pc.parameters[i]);
            pc.curve->GetIndex(static_cast<int>(i)) = pc.indices[i];
          }
        }
      }
      for (const auto& saved : trial.faces) {
        BRep_Builder().UpdateFace(saved.face->GetFace(), saved.triangulation);
        saved.face->UnsetStatus(static_cast<IMeshData_Status>(saved.face->GetStatusMask()));
        saved.face->SetStatus(static_cast<IMeshData_Status>(saved.status));
        for (int wi = 0; wi < saved.face->WiresNb(); ++wi) {
          const auto& wire = saved.face->GetWire(wi);
          wire->UnsetStatus(static_cast<IMeshData_Status>(wire->GetStatusMask()));
          wire->SetStatus(static_cast<IMeshData_Status>(saved.wire_statuses[wi]));
        }
        for (const auto& pc : saved.boundary_indices)
          for (std::size_t i = 0; i < pc.second.size(); ++i)
            pc.first->GetIndex(static_cast<int>(i)) = pc.second[i];
      }
    } catch (...) { throw StripRollbackFailure(); }
  }

  // Certify a simple discrete polygon independently of FaceChecker's small-
  // angle/loop-area exemptions. Endpoints remain the shared CAD mesh nodes.
  static int certified_strip_orientation(const gp_Pnt2d& p,const gp_Pnt2d& q,const gp_Pnt2d& r) {
      const double x=q.X()-p.X(),y=q.Y()-p.Y(),u=r.X()-p.X(),v=r.Y()-p.Y();
      const double scale=std::abs(p.X())+std::abs(p.Y())+std::abs(q.X())+std::abs(q.Y())+
          std::abs(r.X())+std::abs(r.Y());
      const double error=64.0*std::numeric_limits<double>::epsilon()*(std::abs(x*v)+std::abs(y*u)+
          scale*(std::abs(x)+std::abs(y)+std::abs(u)+std::abs(v)));
      const double value=x*v-y*u;
      return std::isfinite(value) && std::isfinite(error) ? (value>error ? 1 : value<-error ? -1 : 0) : 0;
  }
  static bool certified_strip_pair(const gp_Pnt2d& a,const gp_Pnt2d& b,
                                   const gp_Pnt2d& c,const gp_Pnt2d& d,bool adjacent) {
    if (adjacent) {
      gp_Pnt2d before,shared,after;
      if (b.X()==c.X() && b.Y()==c.Y()) { before=a;shared=b;after=d; }
      else if (d.X()==a.X() && d.Y()==a.Y()) { before=c;shared=d;after=b; }
      else return false;
      if (certified_strip_orientation(before,shared,after)!=0) return true;
      const auto incoming=shared.Coord()-before.Coord(),outgoing=after.Coord()-shared.Coord();
      const double value=incoming.Dot(outgoing);
      const double scale=std::abs(before.X())+std::abs(before.Y())+std::abs(shared.X())+std::abs(shared.Y())+
          std::abs(after.X())+std::abs(after.Y());
      const double error=64.0*std::numeric_limits<double>::epsilon()*(
          std::abs(incoming.X()*outgoing.X())+std::abs(incoming.Y()*outgoing.Y())+
          scale*(std::abs(incoming.X())+std::abs(incoming.Y())+std::abs(outgoing.X())+std::abs(outgoing.Y())));
      // Even if the turn is numerically unresolved, rays directed away from
      // their common vertex cannot overlap when this dot product is positive.
      return std::isfinite(value) && std::isfinite(error) && value>error;
    }
    if (std::max(a.X(),b.X())<std::min(c.X(),d.X()) || std::max(c.X(),d.X())<std::min(a.X(),b.X()) ||
        std::max(a.Y(),b.Y())<std::min(c.Y(),d.Y()) || std::max(c.Y(),d.Y())<std::min(a.Y(),b.Y())) return true;
    const int first=certified_strip_orientation(a,b,c),second=certified_strip_orientation(a,b,d);
    if (first!=0 && first==second) return true;
    const int third=certified_strip_orientation(c,d,a),fourth=certified_strip_orientation(c,d,b);
    return third!=0 && third==fourth;
  }

  static bool simple_strip_boundary(IMeshData::IFacePtr face, double& signed_area,bool strict=false,std::string* failure=nullptr) {
    const auto reject=[&](const std::string& reason) { if (failure) *failure=reason;return false; };
    std::vector<gp_Pnt2d> polygon;
    if (face->WiresNb() != 1) return reject("wire count");
    const auto& wire = face->GetWire(0);
    for (int ei = 0; ei < wire->EdgesNb(); ++ei) {
      const auto& edge = wire->GetEdge(ei);
      const auto& pc = edge->GetPCurve(face, wire->GetEdgeOrientation(ei));
      if (pc.IsNull() || pc->ParametersNb() < 2 || polygon.size() + pc->ParametersNb() > 1024) return reject("PCurve/sample budget");
      for (int i = 0; i < pc->ParametersNb() - 1; ++i) {
        const int index = wire->GetEdgeOrientation(ei) == TopAbs_REVERSED ? pc->ParametersNb() - 1 - i : i;
        if (!strip_finite(pc->GetPoint(index))) return reject("nonfinite UV sample");
        polygon.push_back(pc->GetPoint(index));
      }
      const int end = wire->GetEdgeOrientation(ei) == TopAbs_REVERSED ? 0 : pc->ParametersNb() - 1;
      const auto& next_edge = wire->GetEdge((ei + 1) % wire->EdgesNb());
      const auto next_orientation = wire->GetEdgeOrientation((ei + 1) % wire->EdgesNb());
      const auto& next_pc = next_edge->GetPCurve(face, next_orientation);
      if (next_pc.IsNull() || next_pc->ParametersNb() < 2 || pc->GetPoint(end).Distance(
              next_pc->GetPoint(next_orientation == TopAbs_REVERSED ? next_pc->ParametersNb() - 1 : 0)) >
          Precision::PConfusion()) return reject("native wire UV endpoint correspondence edge "+std::to_string(ei));
    }
    signed_area = 0.0;
    if (polygon.size() < 3) return reject("polygon station count");
    const auto origin = polygon.front().Coord();
    for (std::size_t i = 0; i < polygon.size(); ++i) {
      const auto& a = polygon[i]; const auto& b = polygon[(i + 1) % polygon.size()];
      if (a.Distance(b) <= Precision::PConfusion()) return reject("UV segment below PConfusion index "+std::to_string(i));
      signed_area += 0.5 * (a.Coord() - origin).Crossed(b.Coord() - origin);
      for (std::size_t j = i + (strict ? 1 : 2); j < polygon.size(); ++j) {
        const bool adjacent=j==i+1 || (i==0 && j+1==polygon.size());
        if (!strict && adjacent) continue;
        gp_Pnt2d intersection;
        if (strict) {
          const auto& c=polygon[j];const auto& d=polygon[(j+1)%polygon.size()];
          // OCCT classifyPoint uses an unscaled cross-product cutoff. For a
          // tiny cap that can report Glued for genuinely separated segments.
          // Prove separation from stored coordinates instead; unresolved
          // predicates and every true nonadjacent contact remain rejected.
          if (certified_strip_pair(a,b,c,d,adjacent)) continue;
          const auto status=BRepMesh_GeomTool::IntSegSeg(a.Coord(),b.Coord(),c.Coord(),d.Coord(),true,true,intersection);
          std::ostringstream reason;reason.precision(9);
          reason << "uncertified boundary pair " << i << '/' << j << " SDK-status " << static_cast<int>(status) <<
              " adjacent " << adjacent << " lengths " << a.Distance(b) << '/' << c.Distance(d) <<
              " cross " << (b.Coord()-a.Coord()).Crossed(c.Coord()-a.Coord()) << '/' <<
              (b.Coord()-a.Coord()).Crossed(d.Coord()-a.Coord()) << " UV " <<
              a.X() << ',' << a.Y() << ':' << b.X() << ',' << b.Y() << ':' << c.X() << ',' << c.Y() << ':' << d.X() << ',' << d.Y();
          return reject(reason.str());
        }
        const auto status=BRepMesh_GeomTool::IntSegSeg(a.Coord(), b.Coord(), polygon[j].Coord(),
                polygon[(j + 1) % polygon.size()].Coord(), false, false, intersection);
        if (status==BRepMesh_GeomTool::Cross) return reject("crossing segments "+std::to_string(i)+'/'+std::to_string(j));
      }
    }
    return std::isfinite(signed_area) && signed_area > 0.0 ? true : reject("nonpositive/nonfinite signed area "+std::to_string(signed_area));
  }

  bool prepare_spherical_strip(const IMeshData::IFaceHandle& face,
                              const std::set<IMeshData::IFacePtr>& occupied,
                              StripTrial& trial, int& attempts) {
    strip_stop_.clear();
    bool mutated = false;
    const auto reject = [&](const char* reason) {
      std::ostringstream certificate;
      certificate.precision(6);
      certificate << reason << " shift/angular/band/gap " << trial.shift << '/' << trial.angular << '/' <<
          trial.coverage << '/' << trial.neighbor_gap;
      strip_stop_ = certificate.str();
      if (mutated) restore_spherical_strip(trial);
      return false;
    };
    try {
      const auto& wire = face->GetWire(0);
      const auto& surface = face->GetSurface();
      const double radius = surface->Sphere().Radius(), deflection = GetParameters().Deflection;
      if (wire->EdgesNb()==3) {
        std::vector<std::pair<double,int>> spans;
        for (int ei=0;ei<3;++ei) {
          const auto edge=wire->GetEdge(ei);const auto pc=edge->GetPCurve(face.get(),wire->GetEdgeOrientation(ei));
          if (pc.IsNull() || pc->ParametersNb()<2 || pc->ParametersNb()>128 || BRep_Tool::Degenerated(edge->GetEdge()) ||
              BRepAdaptor_Curve(edge->GetEdge()).GetType()!=GeomAbs_Circle || !edge->GetSameParam() || !edge->GetSameRange())
            return reject("three-edge strip lacks precise native circle constraints");
          spans.emplace_back(std::abs(pc->GetPoint(pc->ParametersNb()-1).X()-pc->GetPoint(0).X()),ei);
        }
        std::sort(spans.begin(),spans.end(),[](const auto& a,const auto& b) { return a>b; });
        trial.target_edges={spans[0].second,spans[1].second};trial.connector=spans[2].second;
        if (!std::isfinite(spans[2].first) || spans[2].first<=0.0 || spans[2].first>=spans[1].first/64.0 ||
            radius*spans[2].first>deflection/16.0) return reject("connector is not a bounded small source cap");
        const auto connector=wire->GetEdge(trial.connector);
        const auto pc=connector->GetPCurve(face.get(),wire->GetEdgeOrientation(trial.connector));
        if (pc->ParametersNb()>16 || pc->ParametersNb()!=connector->GetCurve()->ParametersNb())
          return reject("connector native sample correspondence/budget");
      }
      const auto e0 = wire->GetEdge(trial.target_edges[0]), e1 = wire->GetEdge(trial.target_edges[1]);
      if (e0 == e1) return reject("same edge occurs twice in strip");
      if (!e0->GetSameParam() || !e1->GetSameParam() || !e0->GetSameRange() || !e1->GetSameRange()) {
        const std::string flags = "native SameParam/Range e0 " + std::to_string(e0->GetSameParam()) + '/' +
            std::to_string(e0->GetSameRange()) + " e1 " + std::to_string(e1->GetSameParam()) + '/' +
            std::to_string(e1->GetSameRange());
        return reject(flags.c_str());
      }
      const auto p0 = e0->GetPCurve(face.get(), wire->GetEdgeOrientation(trial.target_edges[0]));
      const auto p1 = e1->GetPCurve(face.get(), wire->GetEdgeOrientation(trial.target_edges[1]));
      BRepAdaptor_Curve native0(e0->GetEdge()), native1(e1->GetEdge());
      if (p0.IsNull() || p1.IsNull() || native0.GetType() != GeomAbs_Circle ||
          native1.GetType() != GeomAbs_Circle || p0->ParametersNb() < 3 || p1->ParametersNb() < 3 ||
          p0->ParametersNb() > 128 || p1->ParametersNb() > 128 || !std::isfinite(radius) || radius <= 0.0 ||
          !std::isfinite(deflection) || deflection <= 0.0) return reject("native circle/sample or finite mesh-parameter gate");
      const double edge_tol = std::min(BRep_Tool::Tolerance(e0->GetEdge()), BRep_Tool::Tolerance(e1->GetEdge()));
      double u0, u1, v0, v1; BRepTools::UVBounds(face->GetFace(), u0, u1, v0, v1);
      if (!std::isfinite(edge_tol) || edge_tol <= 0.0 || !std::isfinite(u0) || !std::isfinite(u1) ||
          !std::isfinite(v0) || !std::isfinite(v1) || u1 <= u0 || v1 < v0 || u1 - u0 >= kPi ||
          std::max(std::abs(v0), std::abs(v1)) >= kPi / 4.0 ||
          radius * (v1 - v0) > std::min(deflection, edge_tol) / 16.0) return reject("source UV band or recorded-tolerance gate");
      TopoDS_Vertex a0, a1, b0, b1;
      TopExp::Vertices(e0->GetEdge(), a0, a1); TopExp::Vertices(e1->GetEdge(), b0, b1);
      if (a0.IsNull() || a1.IsNull() || b0.IsNull() || b1.IsNull() || a0.IsSame(a1) || b0.IsSame(b1))
        return reject("distinct native side endpoints were not established");
      if (trial.connector<0) {
        if (!((a0.IsSame(b0) && a1.IsSame(b1)) || (a0.IsSame(b1) && a1.IsSame(b0))))
          return reject("two distinct shared topological endpoints were not established");
      } else {
        TopoDS_Vertex shared;if (!TopExp::CommonVertex(e0->GetEdge(),e1->GetEdge(),shared))
          return reject("connector strip lacks its shared native endpoint");
        const auto other0=a0.IsSame(shared) ? a1 : a0,other1=b0.IsSame(shared) ? b1 : b0;
        TopoDS_Vertex c0,c1;TopExp::Vertices(wire->GetEdge(trial.connector)->GetEdge(),c0,c1);
        if (other0.IsSame(other1) || c0.IsNull() || c1.IsNull() ||
            !((c0.IsSame(other0) && c1.IsSame(other1)) || (c1.IsSame(other0) && c0.IsSame(other1))))
          return reject("unchanged connector does not join both native side ends");
        const double low0=std::min(p0->GetPoint(0).X(),p0->GetPoint(p0->ParametersNb()-1).X());
        const double low1=std::min(p1->GetPoint(0).X(),p1->GetPoint(p1->ParametersNb()-1).X());
        const auto low_vertex0=p0->GetPoint(0).X()<p0->GetPoint(p0->ParametersNb()-1).X() ? a0 : a1;
        const auto low_vertex1=p1->GetPoint(0).X()<p1->GetPoint(p1->ParametersNb()-1).X() ? b0 : b1;
        if (!low_vertex0.IsSame(shared) || !low_vertex1.IsSame(shared) || std::abs(low0-low1)>Precision::PConfusion())
          return reject("connector strip is not an upper cap on a shared U extent");
      }
      const double direction0 = p0->GetPoint(p0->ParametersNb()-1).X() - p0->GetPoint(0).X();
      const double direction1 = p1->GetPoint(p1->ParametersNb()-1).X() - p1->GetPoint(0).X();
      const double traversal0 = direction0 * (wire->GetEdgeOrientation(trial.target_edges[0]) == TopAbs_REVERSED ? -1.0 : 1.0);
      const double traversal1 = direction1 * (wire->GetEdgeOrientation(trial.target_edges[1]) == TopAbs_REVERSED ? -1.0 : 1.0);
      if (!std::isfinite(traversal0) || !std::isfinite(traversal1) || traversal0 * traversal1 >= 0.0)
        return reject("opposed monotone wire traversal was not established");
      // Require a real coincident native-curve crossing, not an arbitrary thin face.
      bool crossing = false;
      for (int i = 1; i < p0->ParametersNb() && !crossing; ++i) for (int j = 1; j < p1->ParametersNb() && !crossing; ++j) {
        if (++strip_comparisons_ > 65536) return reject("spherical crossing comparison budget exhausted");
        gp_Pnt2d uv;
        if (BRepMesh_GeomTool::IntSegSeg(p0->GetPoint(i-1).Coord(), p0->GetPoint(i).Coord(),
                p1->GetPoint(j-1).Coord(), p1->GetPoint(j).Coord(), false, false, uv) !=
            BRepMesh_GeomTool::Cross) continue;
        const auto parameter = [&](const IMeshData::IPCurveHandle& pc, int k) {
          const auto delta = pc->GetPoint(k).Coord() - pc->GetPoint(k-1).Coord();
          return pc->GetParameter(k-1) + (pc->GetParameter(k) - pc->GetParameter(k-1)) *
              (uv.Coord() - pc->GetPoint(k-1).Coord()).Dot(delta) / delta.SquareModulus();
        };
        const double a = parameter(p0, i), b = parameter(p1, j);
        if (!std::isfinite(a) || !std::isfinite(b)) continue;
        if (native0.Value(a).Distance(native1.Value(b))<=Precision::Confusion()) { crossing=true;break; }
        if (!native_export_recovery_) continue;
        // A native 3D curve and its stored source PCurve can differ by their
        // recorded representation tolerance. Establish an actual continuous
        // source-chart crossing before considering that discrepancy; a chord
        // crossing or expanded confusion epsilon is not sufficient.
        if (++strip_continuous_checks_>16) return reject("continuous strip intersection budget exhausted");
        double source_first0,source_last0,source_first1,source_last1;
        const auto source0=BRep_Tool::CurveOnSurface(TopoDS::Edge(e0->GetEdge().Oriented(p0->GetOrientation())),
            face->GetFace(),source_first0,source_last0);
        const auto source1=BRep_Tool::CurveOnSurface(TopoDS::Edge(e1->GetEdge().Oriented(p1->GetOrientation())),
            face->GetFace(),source_first1,source_last1);
        if (source0.IsNull() || source1.IsNull()) return reject("continuous strip source PCurve is unavailable");
        const double a0=std::min(p0->GetParameter(i-1),p0->GetParameter(i)),a1=std::max(p0->GetParameter(i-1),p0->GetParameter(i));
        const double b0=std::min(p1->GetParameter(j-1),p1->GetParameter(j)),b1=std::max(p1->GetParameter(j-1),p1->GetParameter(j));
        if (!std::isfinite(a0) || !std::isfinite(a1) || !std::isfinite(b0) || !std::isfinite(b1) ||
            !std::isfinite(source_first0) || !std::isfinite(source_last0) || !std::isfinite(source_first1) || !std::isfinite(source_last1) ||
            a0>=a1 || b0>=b1 || a0<source_first0 || a1>source_last0 || b0<source_first1 || b1>source_last1)
          return reject("continuous strip intervals leave the exact source range");
        const Handle(Geom2d_Curve) first=new Geom2d_TrimmedCurve(source0,a0,a1,true,false);
        const Handle(Geom2d_Curve) second=new Geom2d_TrimmedCurve(source1,b0,b1,true,false);
        Geom2dAPI_InterCurveCurve exact(first,second,Precision::PConfusion());
        const auto& intersections=exact.Intersector();
        if (!intersections.IsDone() || intersections.NbPoints()!=1 || intersections.NbSegments()!=0) {
          std::ostringstream reason;reason << "continuous strip crossing done/points/segments " << intersections.IsDone();
          if (intersections.IsDone()) reason << '/' << intersections.NbPoints() << '/' << intersections.NbSegments();
          return reject(reason.str().c_str());
        }
        const auto& point=intersections.Point(1);const double at=point.ParamOnFirst(),bt=point.ParamOnSecond();
        if (!std::isfinite(at) || !std::isfinite(bt) || at<=a0 || at>=a1 || bt<=b0 || bt>=b1)
          return reject("continuous strip crossing is not interior to the sampled intervals");
        const auto auv=source0->Value(at),buv=source1->Value(bt);
        if (!strip_finite(auv) || !strip_finite(buv) || auv.Distance(buv)>Precision::PConfusion())
          return reject("continuous source PCurve intersection is unresolved");
        const auto ap=surface->Value(auv.X(),auv.Y()),bp=surface->Value(buv.X(),buv.Y());
        const auto an=native0.Value(at),bn=native1.Value(bt);
        const double error0=ap.Distance(an),error1=bp.Distance(bn),pair=an.Distance(bn),source_gap=ap.Distance(bp);
        const double budget0=std::min(BRep_Tool::Tolerance(e0->GetEdge()),deflection/4.0);
        const double budget1=std::min(BRep_Tool::Tolerance(e1->GetEdge()),deflection/4.0);
        if (!strip_finite(ap) || !strip_finite(bp) || !strip_finite(an) || !strip_finite(bn) ||
            !std::isfinite(error0) || !std::isfinite(error1) || !std::isfinite(pair) || !std::isfinite(source_gap) ||
            source_gap>Precision::Confusion() || error0>budget0 || error1>budget1 || pair>std::min(budget0,budget1))
          return reject("continuous crossing exceeds recorded representation or mesh precision");
        std::ostringstream certificate;certificate.precision(9);
        certificate << " continuous-source crossing native-gap/errors " << pair << '/' << error0 << '/' << error1;
        trial.crossing_certificate=certificate.str();crossing=true;break;
      }
      if (!crossing) return reject("no native-coincident boundary crossing");
      ++attempts;
      trial.target = face.get();
      double errors[2] = {}, width = 0.0;
      std::set<IMeshData::IFacePtr> neighbors;
      for (int ei = 0; ei < (trial.connector>=0 ? 3 : 2); ++ei) {
        const int index=ei<2 ? trial.target_edges[ei] : trial.connector;
        const auto edge = wire->GetEdge(index);
        const auto pc = ei<2 ? (ei == 0 ? p0 : p1) : edge->GetPCurve(face.get(),wire->GetEdgeOrientation(index));
        const auto curve = edge->GetCurve();
        if (curve->ParametersNb() != pc->ParametersNb() || edge->PCurvesNb() != 2 ||
            edge->GetPCurve(0)->GetFace() == edge->GetPCurve(1)->GetFace())
          return reject("inconsistent samples or nonmanifold shared edge");
        StripEdge saved{edge, edge->GetStatusMask(), {}, {}, {}};
        BRepAdaptor_Curve native(edge->GetEdge());
        const double direction = pc->GetPoint(pc->ParametersNb()-1).X() - pc->GetPoint(0).X();
        if (std::abs(direction) < Precision::PConfusion()) return reject("ambiguous U direction");
        for (int i = 0; i < curve->ParametersNb(); ++i) {
          const auto point = curve->GetPoint(i); const auto uv = pc->GetPoint(i);
          if (!strip_finite(point) || !strip_finite(uv) || (i > 0 &&
                  (uv.X() - pc->GetPoint(i-1).X()) * direction <= 0.0)) return reject("nonmonotone source U");
          saved.points.push_back(point); saved.parameters.push_back(curve->GetParameter(i));
          if (ei<2 && i > 0 && i + 1 < curve->ParametersNb())
            errors[ei] = std::max(errors[ei], native.Value(curve->GetParameter(i)).Distance(surface->Value(uv.X(), uv.Y())));
          // Separate the opposing float rounding balls, with one additional
          // rounding radius for cross-product arithmetic. No fixed CAD offset.
          double squared_ulp = 0.0;
          for (double coordinate : {point.X(), point.Y(), point.Z()}) {
            const float f = static_cast<float>(coordinate);
            const double ulp = std::abs(static_cast<double>(std::nextafter(f, std::numeric_limits<float>::infinity())) - f);
            squared_ulp += ulp * ulp;
          }
          if (ei<2) width = std::max(width, 8.0 * std::sqrt(squared_ulp) / radius);
        }
        for (int pi = 0; pi < edge->PCurvesNb(); ++pi) {
          const auto& adjacent = edge->GetPCurve(pi);
          if (adjacent->ParametersNb() != curve->ParametersNb()) return reject("noncorresponding adjacent PCurve");
          StripPCurve saved_pc{adjacent, {}, {}, {}};
          for (int i = 0; i < adjacent->ParametersNb(); ++i) {
            saved_pc.points.push_back(adjacent->GetPoint(i));
            saved_pc.parameters.push_back(adjacent->GetParameter(i));
            saved_pc.indices.push_back(adjacent->GetIndex(i));
            if (adjacent->GetParameter(i) != curve->GetParameter(i)) return reject("nonmatching native parameters");
          }
          neighbors.insert(adjacent->GetFace()); saved.pcurves.push_back(std::move(saved_pc));
        }
        trial.edges.push_back(std::move(saved));
      }
      if (!std::isfinite(width) || radius * width >= std::min(edge_tol, deflection) / 4.0 ||
          neighbors.size() > 8) return reject("float separation exceeds approximation budget");
      for (const auto neighbor : neighbors) {
        if (occupied.count(neighbor) || (neighbor->GetStatusMask() & ~(IMeshData_Outdated | IMeshData_Reused)) != 0)
          return reject("neighbor has prior failure or overlapping transaction");
        for (int fi = 0; fi < GetModel()->FacesNb(); ++fi) {
          const auto other = GetModel()->GetFace(fi).get();
          if (other != neighbor && other->GetFace().IsPartner(neighbor->GetFace()))
            return reject("shared face TShape has an uncertified located alias");
        }
        TopLoc_Location location;
        StripFace saved{neighbor, neighbor->GetStatusMask(), {}, BRep_Tool::Triangulation(neighbor->GetFace(), location), {}};
        const int original_index = strip_original_faces_.FindIndex(neighbor->GetFace());
        if (original_index == 0) return reject("missing original face orientation");
        saved.original_orientation = strip_original_faces_.FindKey(original_index).Orientation();
        std::size_t index_count = 0;
        for (int wi = 0; wi < neighbor->WiresNb(); ++wi) {
          saved.wire_statuses.push_back(neighbor->GetWire(wi)->GetStatusMask());
          if (saved.wire_statuses.back() != 0) return reject("neighbor wire has prior failure");
          const auto& adjacent_wire = neighbor->GetWire(wi);
          for (int ei = 0; ei < adjacent_wire->EdgesNb(); ++ei) {
            const auto pc = adjacent_wire->GetEdge(ei)->GetPCurve(neighbor, adjacent_wire->GetEdgeOrientation(ei));
            if (pc.IsNull() || (index_count += pc->ParametersNb()) > 65536) return reject("neighbor index snapshot budget exhausted");
            std::vector<int> indices;
            for (int i = 0; i < pc->ParametersNb(); ++i) indices.push_back(pc->GetIndex(i));
            saved.boundary_indices.push_back({pc, std::move(indices)});
          }
        }
        trial.faces.push_back(std::move(saved));
      }
      const int moved = errors[0] >= errors[1] ? 0 : 1;
      const auto reference = moved == 0 ? p1 : p0;
      const auto moved_pc = moved == 0 ? p0 : p1;
      const double traversal = (moved_pc->GetPoint(moved_pc->ParametersNb()-1).X() - moved_pc->GetPoint(0).X()) *
          (wire->GetEdgeOrientation(trial.target_edges[moved]) == TopAbs_REVERSED ? -1.0 : 1.0);
      const double side = traversal < 0.0 ? 1.0 : -1.0;
      const auto interpolate = [](const std::vector<double>& parameters, const std::vector<gp_Pnt2d>& points, double t) {
        for (std::size_t i = 1; i < parameters.size(); ++i) {
          if ((t - parameters[i-1]) * (t - parameters[i]) > 0.0) continue;
          const double a = (t - parameters[i-1]) / (parameters[i] - parameters[i-1]);
          return gp_Pnt2d(points[i-1].Coord() * (1.0-a) + points[i].Coord() * a);
        }
        throw std::runtime_error("Spherical mesh parameter leaves its original range");
      };
      const auto reference_v = [&](double u) {
        for (int i = 1; i < reference->ParametersNb(); ++i) {
          const auto a = reference->GetPoint(i-1), b = reference->GetPoint(i);
          if ((u - a.X()) * (u - b.X()) > 0.0) continue;
          return a.Y() + (b.Y() - a.Y()) * (u - a.X()) / (b.X() - a.X());
        }
        if (trial.connector>=0) return std::abs(u-reference->GetPoint(0).X())<
            std::abs(u-reference->GetPoint(reference->ParametersNb()-1).X()) ? reference->GetPoint(0).Y() :
            reference->GetPoint(reference->ParametersNb()-1).Y();
        throw std::runtime_error("Spherical mesh U leaves the shared strip");
      };
      const auto corrected_boundary=[&](double u) {
        double scale=1.0;
        if (trial.connector>=0) {
          const double lo=std::min(moved_pc->GetPoint(0).X(),moved_pc->GetPoint(moved_pc->ParametersNb()-1).X());
          const double hi=std::max(moved_pc->GetPoint(0).X(),moved_pc->GetPoint(moved_pc->ParametersNb()-1).X());
          scale=std::min(1.0,std::max(0.0,std::min(u-lo,hi-u))*GetParameters().Angle/(16.0*width));
        }
        return reference_v(u)+side*width*scale;
      };
      double new_v0 = v0, new_v1 = v1, max_sag = 0.0;
      std::array<std::vector<gp_Pnt>, 2> points;
      std::array<std::vector<double>, 2> parameters;
      std::array<std::vector<gp_Pnt2d>, 2> target_uv;
      for (int ei = 0; ei < 2; ++ei) {
        const auto& saved = trial.edges[ei];
        BRepAdaptor_Curve native(saved.edge->GetEdge());
        const auto native_pc = saved.edge->GetPCurve(face.get(), wire->GetEdgeOrientation(trial.target_edges[ei]));
        const auto pc_saved = std::find_if(saved.pcurves.begin(), saved.pcurves.end(),
            [&](const StripPCurve& pc) { return pc.curve == native_pc; });
        if (pc_saved == saved.pcurves.end()) return reject("missing target PCurve snapshot");
        double first, last;
        const auto source_pc = BRep_Tool::CurveOnSurface(saved.edge->GetEdge(), face->GetFace(), first, last);
        if (source_pc.IsNull()) return reject("missing exact target PCurve");
        parameters[ei] = saved.parameters;
        bool certified = false;
        for (int pass = 0; pass < 6 && parameters[ei].size() <= 256; ++pass) {
          points[ei].clear(); target_uv[ei].clear();
          double local_shift = 0.0;
          for (std::size_t i = 0; i < parameters[ei].size(); ++i) {
            const double t = parameters[ei][i];
            auto uv = source_pc->Value(t); const auto seed = interpolate(saved.parameters, pc_saved->points, t);
            uv.SetX(uv.X() + std::round((seed.X() - uv.X()) / kTau) * kTau);
            gp_Pnt point = native.Value(t);
            if (i == 0 || i + 1 == parameters[ei].size()) {
              point = i == 0 ? saved.points.front() : saved.points.back();
              uv = i == 0 ? pc_saved->points.front() : pc_saved->points.back();
            } else if (ei == moved) {
              const double boundary = corrected_boundary(uv.X());
              uv.SetY(side > 0.0 ? std::max(uv.Y(), boundary) : std::min(uv.Y(), boundary));
              point = surface->Value(uv.X(), uv.Y());
            }
            const double shift = point.Distance(native.Value(t));
            if (!strip_finite(point) || !strip_finite(uv) || !std::isfinite(shift) ||
                shift > std::min(BRep_Tool::Tolerance(saved.edge->GetEdge()), deflection / 4.0))
              return reject("shared sample displacement exceeds recorded tolerance");
            local_shift = std::max(local_shift, shift);
            points[ei].push_back(point); target_uv[ei].push_back(uv);
          }
          certified = true;
          double local_angle = 0.0, local_sag = 0.0;
          const double half_angle = 0.5 * std::min(GetParameters().Angle, saved.edge->GetAngularDeflection());
          if (!std::isfinite(half_angle) || half_angle <= 0.0) return reject("invalid angular request");
          for (std::size_t i = 1; i < parameters[ei].size(); ++i) {
            const double step = std::abs(parameters[ei][i] - parameters[ei][i-1]);
            const double chord = native.Value(parameters[ei][i]).Distance(native.Value(parameters[ei][i-1]));
            const double error = points[ei][i].Distance(native.Value(parameters[ei][i])) +
                points[ei][i-1].Distance(native.Value(parameters[ei][i-1]));
            const double angle = chord > error ? step + 2.0 * std::asin(error / chord) : std::numeric_limits<double>::infinity();
            const double sag = native.Circle().Radius() * (1.0 - std::cos(step / 2.0)) + local_shift;
            certified &= std::isfinite(angle) && angle <= half_angle && std::isfinite(sag) && sag <= deflection;
            local_angle = std::max(local_angle, angle); local_sag = std::max(local_sag, sag);
          }
          if (certified) {
            trial.shift = std::max(trial.shift, local_shift); trial.angular = std::max(trial.angular, local_angle);
            max_sag = std::max(max_sag, local_sag); break;
          }
          if (parameters[ei].size() * 2 - 1 > 256) break;
          std::vector<double> refined;
          for (std::size_t i = 1; i < parameters[ei].size(); ++i) {
            refined.push_back(parameters[ei][i-1]);
            refined.push_back(0.5 * (parameters[ei][i-1] + parameters[ei][i]));
          }
          refined.push_back(parameters[ei].back()); parameters[ei] = std::move(refined);
        }
        if (!certified) return reject("half-Angle or circle-chord certificate exhausted");
        const double direction = target_uv[ei].back().X() - target_uv[ei].front().X();
        for (std::size_t i = 0; i < target_uv[ei].size(); ++i) {
          const auto& uv = target_uv[ei][i];
          if (uv.X() < u0 - Precision::PConfusion() || uv.X() > u1 + Precision::PConfusion() ||
              (i && (uv.X() - target_uv[ei][i-1].X()) * direction <= 0.0))
            return reject("new samples leave monotone source U extent");
        }
        if (trial.connector<0 && (std::abs(std::min(target_uv[ei].front().X(), target_uv[ei].back().X()) - u0) > Precision::PConfusion() ||
            std::abs(std::max(target_uv[ei].front().X(), target_uv[ei].back().X()) - u1) > Precision::PConfusion()))
          return reject("shared endpoints do not cover exact source U extent");
        for (const auto& uv : target_uv[ei]) { new_v0 = std::min(new_v0, uv.Y()); new_v1 = std::max(new_v1, uv.Y()); }
      }
      // Pair both rails at every original U station. This prevents a native
      // triangulator from spanning a thin strip with a three-point circle ear.
      if (native_export_recovery_) {
        std::vector<double> stations;
        for (const auto& edge_uv : target_uv) for (const auto& uv : edge_uv) stations.push_back(uv.X());
        std::sort(stations.begin(),stations.end());stations.erase(std::unique(stations.begin(),stations.end()),stations.end());
        if (stations.size()<3 || stations.size()>256) return reject("matched U station budget exhausted");
        for (int ei=0;ei<2;++ei) {
          const auto& saved=trial.edges[ei];BRepAdaptor_Curve native(saved.edge->GetEdge());
          const auto native_pc=saved.edge->GetPCurve(face.get(),wire->GetEdgeOrientation(trial.target_edges[ei]));
          const auto pc_saved=std::find_if(saved.pcurves.begin(),saved.pcurves.end(),
              [&](const StripPCurve& pc) { return pc.curve==native_pc; });
          double first,last;
          const auto source_pc=BRep_Tool::CurveOnSurface(TopoDS::Edge(saved.edge->GetEdge().Oriented(native_pc->GetOrientation())),
              face->GetFace(),first,last);
          if (source_pc.IsNull() || pc_saved==saved.pcurves.end()) return reject("matched strip source chart is unavailable");
          const auto original_parameters=parameters[ei];const auto original_uv=target_uv[ei];
          const auto evaluate=[&](double t) {
            if (++export_boundary_work_>2097152) throw std::runtime_error("matched station source budget");
            auto uv=source_pc->Value(t);const auto seed=interpolate(saved.parameters,pc_saved->points,t);
            uv.SetX(uv.X()+std::round((seed.X()-uv.X())/kTau)*kTau);
            if (!strip_finite(uv)) throw std::runtime_error("nonfinite matched source UV");return uv;
          };
          auto ordered=stations;
          if (trial.connector>=0) {
            const double lo=std::min(original_uv.front().X(),original_uv.back().X());
            const double hi=std::max(original_uv.front().X(),original_uv.back().X());
            ordered.erase(std::remove_if(ordered.begin(),ordered.end(),[&](double u) { return u<lo || u>hi; }),ordered.end());
          }
          if (original_uv.front().X()>original_uv.back().X()) std::reverse(ordered.begin(),ordered.end());
          std::vector<double> matched_parameters;std::vector<gp_Pnt2d> matched_uv;std::vector<gp_Pnt> matched_points;
          for (double station : ordered) {
            double t=std::numeric_limits<double>::quiet_NaN();
            for (std::size_t i=0;i<original_uv.size();++i) if (original_uv[i].X()==station) { t=original_parameters[i];break; }
            if (!std::isfinite(t)) {
              for (std::size_t i=1;i<original_uv.size();++i) {
                const double left_u=original_uv[i-1].X(),right_u=original_uv[i].X();
                if ((station-left_u)*(station-right_u)>=0.0) continue;
                double left=original_parameters[i-1],right=original_parameters[i];
                for (int pass=0;pass<56;++pass) {
                  const double middle=.5*(left+right),u=evaluate(middle).X();
                  if ((u-station)*(right_u-left_u)<0.0) left=middle;else right=middle;
                }
                t=.5*(left+right);break;
              }
            }
            if (!std::isfinite(t)) return reject("matched U station leaves original parameter range");
            auto uv=evaluate(t);gp_Pnt point=native.Value(t);
            const bool endpoint=matched_parameters.empty() || matched_parameters.size()+1==ordered.size();
            if (endpoint) {
              uv=matched_parameters.empty() ? pc_saved->points.front() : pc_saved->points.back();
              point=matched_parameters.empty() ? saved.points.front() : saved.points.back();
            } else {
              if (std::abs(uv.X()-station)>64*std::numeric_limits<double>::epsilon()*std::max(1.0,std::abs(station)))
                return reject("matched source inverse did not converge");
              uv.SetX(station);
              if (ei==moved) {
                const double boundary=corrected_boundary(station);
                uv.SetY(side>0.0 ? std::max(uv.Y(),boundary) : std::min(uv.Y(),boundary));point=surface->Value(uv.X(),uv.Y());
              }
            }
            const double shift=point.Distance(native.Value(t));
            if (!strip_finite(point) || !std::isfinite(shift) || shift>std::min(BRep_Tool::Tolerance(saved.edge->GetEdge()),deflection/4.0))
              return reject("matched shared sample exceeds recorded displacement budget");
            trial.shift=std::max(trial.shift,shift);new_v0=std::min(new_v0,uv.Y());new_v1=std::max(new_v1,uv.Y());
            if (!matched_parameters.empty() && t<=matched_parameters.back()) return reject("matched native parameters are not strictly ordered");
            matched_parameters.push_back(t);matched_uv.push_back(uv);matched_points.push_back(point);
          }
          const double half_angle=.5*std::min(GetParameters().Angle,saved.edge->GetAngularDeflection());
          if (!std::isfinite(half_angle) || half_angle<=0.0) return reject("invalid matched circle angular request");
          for (std::size_t i=1;i<matched_parameters.size();++i) {
            const double step=matched_parameters[i]-matched_parameters[i-1];
            const gp_Vec native_chord(native.Value(matched_parameters[i-1]),native.Value(matched_parameters[i]));
            const gp_Vec chord(matched_points[i-1],matched_points[i]);
            if (!std::isfinite(native_chord.SquareMagnitude()) || native_chord.SquareMagnitude()<=0.0 ||
                !std::isfinite(chord.SquareMagnitude()) || chord.SquareMagnitude()<=0.0) return reject("matched curve chord is degenerate");
            // Actual displacement differences, rather than independent error
            // balls, certify the nearly coincident matched stations. For a
            // circle the tangent/chord angle is bounded by half its arc step.
            const double angular=step+2.0*chord.Angle(native_chord);
            const double sag=native.Circle().Radius()*(1.0-std::cos(step/2.0))+trial.shift;
            if (!std::isfinite(angular) || angular>half_angle || !std::isfinite(sag) || sag>deflection)
              return reject("matched curve half-Angle or circle-sag certificate failed");
            trial.angular=std::max(trial.angular,angular);max_sag=std::max(max_sag,sag);
          }
          parameters[ei]=std::move(matched_parameters);points[ei]=std::move(matched_points);target_uv[ei]=std::move(matched_uv);
        }
      }
      // The full regions cover the same monotone-U extent in a nonperiodic
      // chart, including the unchanged connector and unmatched side tails.
      // Latitude-band distance plus chord interpolation bounds both directions,
      // including the original crossing lobes, without assuming equal area.
      trial.coverage = radius * (new_v1 - new_v0) + max_sag;
      if (trial.connector>=0) {
        const auto pc=wire->GetEdge(trial.connector)->GetPCurve(face.get(),wire->GetEdgeOrientation(trial.connector));
        double lower=std::numeric_limits<double>::infinity(),upper=-lower;
        for (const auto& samples : target_uv) for (const auto& uv : samples) { lower=std::min(lower,uv.X());upper=std::max(upper,uv.X()); }
        const double direction=pc->GetPoint(pc->ParametersNb()-1).X()-pc->GetPoint(0).X();
        for (int i=0;i<pc->ParametersNb();++i) {
          const auto uv=pc->GetPoint(i);
          if (!strip_finite(uv) || uv.Y()<v0-Precision::PConfusion() || uv.Y()>v1+Precision::PConfusion() ||
              (i && (uv.X()-pc->GetPoint(i-1).X())*direction<=0.0)) return reject("connector leaves monotone original source band");
          lower=std::min(lower,uv.X());upper=std::max(upper,uv.X());
        }
        if (std::abs(lower-u0)>Precision::PConfusion() || std::abs(upper-u1)>Precision::PConfusion())
          return reject("complete connector strip does not cover source U extent");
      }
      if (!std::isfinite(trial.coverage) || trial.coverage > deflection) return reject("two-way strip coverage exceeds deflection");
      mutated = true;
      for (int ei = 0; ei < 2; ++ei) {
        const auto& saved = trial.edges[ei]; const auto& curve = saved.edge->GetCurve();
        curve->Clear(false);
        for (std::size_t i = 0; i < parameters[ei].size(); ++i) curve->AddPoint(points[ei][i], parameters[ei][i]);
        saved.edge->SetStatus(IMeshData_Outdated);
        for (const auto& pc : saved.pcurves) {
          const auto adjacent = pc.curve->GetFace();
          pc.curve->Clear(false);
          for (std::size_t i = 0; i < parameters[ei].size(); ++i) {
            auto uv = interpolate(pc.parameters, pc.points, parameters[ei][i]);
            if (i == 0 || i + 1 == parameters[ei].size()) uv = i == 0 ? pc.points.front() : pc.points.back();
            else if (adjacent == face.get()) uv = target_uv[ei][i];
            // On every other face retain its healed discrete UV polygon.
            // Inserted native parameters only subdivide those saved segments;
            // no inverse projection can jump sheets or undo junction repairs.
            // The shared 3D point may differ from that surface point only within
            // the recorded CAD tolerance and a quarter of the mesh error budget.
            if (!strip_finite(uv)) return reject("nonfinite retained boundary UV");
            const auto on_surface = adjacent->GetSurface()->Value(uv.X(), uv.Y());
            const double gap = on_surface.Distance(points[ei][i]);
            if (std::isfinite(gap)) trial.neighbor_gap = std::max(trial.neighbor_gap, gap);
            if (!strip_finite(on_surface) || !std::isfinite(gap) || gap > std::min(deflection / 4.0,
                    BRep_Tool::Tolerance(saved.edge->GetEdge()) + BRep_Tool::Tolerance(adjacent->GetFace())))
              return reject("adjacent surface mismatch exceeds recorded tolerance");
            pc.curve->AddPoint(uv, parameters[ei][i]);
          }
        }
      }
      for (const auto& saved : trial.faces) {
        saved.face->UnsetStatus(IMeshData_Reused);
        saved.face->SetStatus(IMeshData_Outdated);
        BRepMesh_FaceChecker checker(saved.face, GetParameters());
        if (!checker.Perform()) return reject("adjacent FaceChecker rejected regularized boundary");
      }
      double area;
      std::string boundary_stop;
      if (!simple_strip_boundary(face.get(), area,trial.connector>=0,&boundary_stop))
        return reject(("regularized strip boundary: "+boundary_stop).c_str());
      return true;
    } catch (const StripRollbackFailure&) { throw; }
      catch (const Standard_Failure&) { return reject("OCCT exception preparing regularized strip"); }
      catch (const std::exception&) { return reject("exception preparing regularized strip"); }
  }

  // Native meshing may merge close UV stations even though their native
  // parameters and physical samples are distinct. Restore their correspondence
  // on copied face meshes, without coalescing source stations or junctions.
  bool restore_strip_station_nodes(const StripTrial& trial) {
    try {
      for (const auto& saved : trial.faces) {
        const auto face=saved.face;TopLoc_Location location;
        const auto mesh=BRep_Tool::Triangulation(face->GetFace(),location);
        if (mesh.IsNull() || !mesh->HasUVNodes() || mesh->NbNodes()>65536) {
          strip_stop_="strip station owner has no bounded UV mesh";return false;
        }
        std::set<int> endpoints,other_constraints;
        std::map<int,TopoDS_Vertex> endpoint_vertices;
        std::set<int> ambiguous_vertices;
        for (int wi=0;wi<face->WiresNb();++wi) {
          const auto wire=face->GetWire(wi);
          for (int ei=0;ei<wire->EdgesNb();++ei) {
            const auto edge=wire->GetEdge(ei);const auto pc=edge->GetPCurve(face,wire->GetEdgeOrientation(ei));
            if (pc.IsNull() || pc->ParametersNb()<2) { strip_stop_="strip station owner lacks native constraints";return false; }
            TopoDS_Vertex first,last;TopExp::Vertices(edge->GetEdge(),first,last);
            for (int index : {0,pc->ParametersNb()-1}) {
              const int id=pc->GetIndex(index);endpoints.insert(id);
              const auto vertex=index==0 ? first : last;
              const auto found=endpoint_vertices.find(id);
              if (vertex.IsNull() || (found!=endpoint_vertices.end() && !found->second.IsSame(vertex))) ambiguous_vertices.insert(id);
              else endpoint_vertices[id]=vertex;
            }
            const bool shared=std::any_of(trial.edges.begin(),trial.edges.end(),[&](const StripEdge& item) { return item.edge==edge; });
            if (!shared) for (int i=1;i+1<pc->ParametersNb();++i) other_constraints.insert(pc->GetIndex(i));
          }
        }
        struct Station { IMeshData::IPCurveHandle pc;int index,old;gp_Pnt2d uv;gp_Pnt point;double budget;bool endpoint; };
        std::vector<Station> stations;std::map<int,std::vector<std::size_t>> groups;
        for (const auto& edge : trial.edges) for (const auto& pc : edge.pcurves) {
          if (pc.curve->GetFace()!=face) continue;
          const auto curve=edge.edge->GetCurve();
          if (pc.curve->ParametersNb()!=curve->ParametersNb() || curve->ParametersNb()>256) {
            strip_stop_="strip station owner parameter correspondence";return false;
          }
          for (int i=0;i<curve->ParametersNb();++i) {
            if (++export_boundary_work_>2097152) { strip_stop_="strip station reconstruction work budget";return false; }
            const int old=pc.curve->GetIndex(i);
            const auto uv=pc.curve->GetPoint(i);const auto point=curve->GetPoint(i);
            const double budget=std::min(GetParameters().Deflection/4.0,
                BRep_Tool::Tolerance(edge.edge->GetEdge())+BRep_Tool::Tolerance(face->GetFace()));
            if (old<1 || old>mesh->NbNodes() || !strip_finite(uv) || !strip_finite(point) || !std::isfinite(budget) || budget<=0.0) {
              strip_stop_="strip station reconstruction invalid index/point/budget";return false;
            }
            const bool endpoint=i==0 || i+1==curve->ParametersNb();
            if (endpoint && (ambiguous_vertices.count(old) || !endpoints.count(old) ||
                mesh->Node(old).Transformed(location.Transformation()).Distance(point)>Precision::Confusion() ||
                mesh->UVNode(old).Distance(uv)>Precision::PConfusion())) {
              strip_stop_="strip station native endpoint ownership/correspondence";return false;
            }
            groups[old].push_back(stations.size());stations.push_back({pc.curve,i,old,uv,point,budget,endpoint});
          }
        }
        const auto replacement=mesh->Copy();
        for (const auto& group : groups) {
          const int old=group.first;bool native_endpoint=endpoints.count(old)!=0;
          std::size_t retained=stations.size();double best=std::numeric_limits<double>::infinity();
          const auto old_point=mesh->Node(old).Transformed(location.Transformation());const auto old_uv=mesh->UVNode(old);
          if (!strip_finite(old_point) || !strip_finite(old_uv)) { strip_stop_="strip station original node is nonfinite";return false; }
          for (std::size_t index : group.second) {
            const auto& station=stations[index];
            if (station.endpoint) continue;
            if (native_endpoint || other_constraints.count(old)) continue;
            const auto before=face->GetSurface()->Value(old_uv.X(),old_uv.Y());
            const auto after=face->GetSurface()->Value(station.uv.X(),station.uv.Y());
            const double shift=old_point.Distance(station.point),chart_shift=before.Distance(after);
            if (!strip_finite(before) || !strip_finite(after) || !std::isfinite(shift) || !std::isfinite(chart_shift) ||
                shift>station.budget || chart_shift>station.budget) continue;
            if (shift+chart_shift<best) { best=shift+chart_shift;retained=index; }
          }
          for (std::size_t index : group.second) {
            auto& station=stations[index];
            if (station.endpoint) continue;
            int node=old;
            if (index!=retained) {
              if (replacement->NbNodes()>=65536) { strip_stop_="strip station reconstructed node budget";return false; }
              node=replacement->NbNodes()+1;replacement->ResizeNodes(node,true);
            }
            const auto local=station.point.Transformed(location.Transformation().Inverted());
            const auto represented=local.Transformed(location.Transformation());
            if (!strip_finite(local) || !strip_finite(represented) || represented.Distance(station.point)>Precision::Confusion()) {
              strip_stop_="strip station local/world correspondence";return false;
            }
            replacement->SetNode(node,local);replacement->SetUVNode(node,station.uv);station.pc->GetIndex(station.index)=node;
          }
        }
        replacement->RemoveNormals();replacement->ComputeNormals();BRep_Builder().UpdateFace(face->GetFace(),replacement);
      }
      return true;
    } catch (const Standard_Failure&) { strip_stop_="OCCT exception reconstructing native strip stations";return false; }
      catch (const std::exception&) { strip_stop_="exception reconstructing native strip stations";return false; }
  }

  bool triangulate_synchronized_strip(const StripTrial& trial) {
    try {
      if (trial.connector>=0) return triangulate_connector_strip(trial);
      if (!trial.target || trial.target->WiresNb()!=1 || trial.target->GetWire(0)->EdgesNb()!=2) {
        strip_stop_="matched strip has no two-edge target";return false;
      }
      const auto wire=trial.target->GetWire(0);
      const auto first=wire->GetEdge(0)->GetPCurve(trial.target,wire->GetEdgeOrientation(0));
      const auto second=wire->GetEdge(1)->GetPCurve(trial.target,wire->GetEdgeOrientation(1));
      if (first.IsNull() || second.IsNull() || first->ParametersNb()!=second->ParametersNb() || first->ParametersNb()>256) {
        strip_stop_="matched strip station counts differ";return false;
      }
      TopLoc_Location location;const auto mesh=BRep_Tool::Triangulation(trial.target->GetFace(),location);
      if (mesh.IsNull() || !mesh->HasUVNodes()) { strip_stop_="matched strip has no indexed boundary mesh";return false; }
      const auto ascending=[](const IMeshData::IPCurveHandle& pc,int station) {
        return pc->GetPoint(0).X()<pc->GetPoint(pc->ParametersNb()-1).X() ? station : pc->ParametersNb()-1-station;
      };
      const int count=first->ParametersNb();std::vector<std::array<int,3>> triangles;
      for (int i=0;i<count;++i) {
        const int a=ascending(first,i),b=ascending(second,i),ai=first->GetIndex(a),bi=second->GetIndex(b);
        if (ai<1 || bi<1 || ai>mesh->NbNodes() || bi>mesh->NbNodes() ||
            first->GetPoint(a).X()!=second->GetPoint(b).X()) { strip_stop_="matched strip indexed U correspondence";return false; }
        if ((i==0 || i+1==count) && ai!=bi) { strip_stop_="matched native endpoint has two mesh representatives";return false; }
        if (i && i+1<count && ai==bi) { strip_stop_="matched strip interior boundaries collapsed";return false; }
        if (!i) continue;
        const int pa=first->GetIndex(ascending(first,i-1)),pb=second->GetIndex(ascending(second,i-1));
        if (pa==ai || pb==bi) { strip_stop_="consecutive native strip stations collapsed";return false; }
        for (auto cell : {std::array<int,3>{pa,ai,bi},std::array<int,3>{pa,bi,pb}}) {
          if (cell[0]==cell[1] || cell[1]==cell[2] || cell[2]==cell[0]) {
            if ((i==1 && pa==pb) || (i+1==count && ai==bi)) continue;
            strip_stop_="non-endpoint matched strip cell collapsed";return false;
          }
          const auto u=mesh->UVNode(cell[0]),v=mesh->UVNode(cell[1]),w=mesh->UVNode(cell[2]);
          const double area=(v.Coord()-u.Coord()).Crossed(w.Coord()-u.Coord());
          if (!std::isfinite(area) || area==0.0) { strip_stop_="matched strip has zero/nonfinite UV cell";return false; }
          if (area<0.0) std::swap(cell[1],cell[2]);triangles.push_back(cell);
        }
      }
      if (triangles.size()!=static_cast<std::size_t>(2*count-4) || triangles.size()>508) { strip_stop_="matched strip facet budget/coverage";return false; }
      const auto replacement=mesh->Copy();replacement->ResizeTriangles(static_cast<int>(triangles.size()),false);
      for (std::size_t i=0;i<triangles.size();++i) replacement->SetTriangle(static_cast<int>(i)+1,
          Poly_Triangle(triangles[i][0],triangles[i][1],triangles[i][2]));
      replacement->RemoveNormals();replacement->ComputeNormals();BRep_Builder().UpdateFace(trial.target->GetFace(),replacement);
      return true;
    } catch (const Standard_Failure&) { strip_stop_="OCCT exception rebuilding matched strip";return false; }
      catch (const std::exception&) { strip_stop_="exception rebuilding matched strip";return false; }
  }

  // Pair the common interval of two long sides, then retain every unchanged
  // connector/tail station in a constrained cap. The complete wire/domain and
  // seven source precision witnesses still qualify all installed cells.
  bool triangulate_connector_strip(const StripTrial& trial) {
    try {
      if (!trial.target || trial.target->WiresNb()!=1 || trial.target->GetWire(0)->EdgesNb()!=3) {
        strip_stop_="connector target is not a three-edge native wire";return false;
      }
      const auto wire=trial.target->GetWire(0);TopLoc_Location location;
      const auto mesh=BRep_Tool::Triangulation(trial.target->GetFace(),location);
      if (mesh.IsNull() || !mesh->HasUVNodes()) { strip_stop_="connector target lacks a UV mesh";return false; }
      std::array<IMeshData::IPCurveHandle,2> sides;
      std::array<std::vector<std::pair<double,int>>,2> stations;
      for (int ei=0;ei<2;++ei) {
        const int index=trial.target_edges[ei];sides[ei]=wire->GetEdge(index)->GetPCurve(trial.target,wire->GetEdgeOrientation(index));
        if (sides[ei].IsNull() || sides[ei]->ParametersNb()>256) { strip_stop_="connector side station budget";return false; }
        const auto pc=sides[ei];
        for (int i=0;i<pc->ParametersNb();++i) stations[ei].emplace_back(pc->GetPoint(i).X(),pc->GetIndex(i));
        if (stations[ei].front().first>stations[ei].back().first) std::reverse(stations[ei].begin(),stations[ei].end());
      }
      std::vector<std::array<int,3>> cells;
      const auto append=[&](std::array<int,3> cell) {
        if (cell[0]==cell[1] || cell[1]==cell[2] || cell[2]==cell[0]) return false;
        for (int id : cell) if (id<1 || id>mesh->NbNodes()) return false;
        const auto a=mesh->UVNode(cell[0]),b=mesh->UVNode(cell[1]),c=mesh->UVNode(cell[2]);
        const double area=(b.Coord()-a.Coord()).Crossed(c.Coord()-a.Coord());
        if (!std::isfinite(area) || area==0.0) return false;
        if (area<0.0) std::swap(cell[1],cell[2]);cells.push_back(cell);return cells.size()<=1024;
      };
      std::vector<std::pair<int,int>> matched;
      std::size_t a=0,b=0;
      // A source chart can encode the very same native vertex with endpoint
      // UVs differing by arithmetic roundoff. Only a separately certified
      // native identity may pair those occurrences without changing either UV.
      if (trial.certified_native_apex) {
        if (stations[0][0].second!=stations[1][0].second) { strip_stop_="certified native apex mesh ownership differs";return false; }
        matched.emplace_back(0,0);a=b=1;
      }
      while (a<stations[0].size() && b<stations[1].size()) {
        if (stations[0][a].first<stations[1][b].first) { ++a;continue; }
        if (stations[1][b].first<stations[0][a].first) { ++b;continue; }
        matched.emplace_back(static_cast<int>(a),static_cast<int>(b));++a;++b;
      }
      if (matched.size()<3 || matched.front()!=std::pair<int,int>{0,0} || stations[0][0].second!=stations[1][0].second) {
        strip_stop_="connector common U stations/native junction mismatch";return false;
      }
      for (std::size_t i=1;i<matched.size();++i) {
        if (matched[i].first!=matched[i-1].first+1 || matched[i].second!=matched[i-1].second+1) {
          strip_stop_="connector common interval contains unmatched original station";return false;
        }
        const int pa=stations[0][matched[i-1].first].second,pb=stations[1][matched[i-1].second].second;
        const int na=stations[0][matched[i].first].second,nb=stations[1][matched[i].second].second;
        if (!append({pa,na,nb}) || (pa!=pb && !append({pa,nb,pb}))) {
          strip_stop_="connector common zipper cell is degenerate";return false;
        }
      }
      const int ca=stations[0][matched.back().first].second,cb=stations[1][matched.back().second].second;
      std::vector<int> boundary;
      for (int ei=0;ei<wire->EdgesNb();++ei) {
        const auto pc=wire->GetEdge(ei)->GetPCurve(trial.target,wire->GetEdgeOrientation(ei));
        if (pc.IsNull() || pc->ParametersNb()>256) { strip_stop_="connector full boundary station budget";return false; }
        for (int i=0;i+1<pc->ParametersNb();++i) boundary.push_back(pc->GetIndex(
            wire->GetEdgeOrientation(ei)==TopAbs_REVERSED ? pc->ParametersNb()-1-i : i));
      }
      if (boundary.size()>528 || std::count(boundary.begin(),boundary.end(),ca)!=1 ||
          std::count(boundary.begin(),boundary.end(),cb)!=1) { strip_stop_="connector cap junction ownership";return false; }
      std::vector<int> cap;
      const auto trace=[&](int start,int stop) {
        std::vector<int> path;auto position=std::find(boundary.begin(),boundary.end(),start)-boundary.begin();
        for (std::size_t i=0;i<boundary.size();++i) {
          const int id=boundary[(position+i)%boundary.size()];path.push_back(id);
          if (id==stop && i) return path;
        }
        return std::vector<int>{};
      };
      auto first=trace(ca,cb),second=trace(cb,ca);
      const auto contains_shared=[&](const std::vector<int>& path) { return std::find(path.begin(),path.end(),stations[0][0].second)!=path.end(); };
      if (contains_shared(first)==contains_shared(second)) { strip_stop_="connector unique cap traversal";return false; }
      cap=contains_shared(first) ? second : first;
      if (cap.size()<3 || cap.size()>32) { strip_stop_="connector native cap constraint budget";return false; }
      // Preserve the cap's oriented boundary, including collinear stations.
      // Only positive ears may be removed from the active polygon, and their
      // triangles retain every original connector/tail constraint.
      double cap_area=0.0;const auto origin=mesh->UVNode(cap.front()).Coord();
      for (std::size_t i=0;i<cap.size();++i) cap_area+=(mesh->UVNode(cap[i]).Coord()-origin).Crossed(
          mesh->UVNode(cap[(i+1)%cap.size()]).Coord()-origin);
      if (!std::isfinite(cap_area) || cap_area<=0.0) { strip_stop_="connector cap UV area/winding";return false; }
      if (std::set<int>(cap.begin(),cap.end()).size()!=cap.size()) { strip_stop_="connector cap repeats a native node";return false; }
      for (std::size_t i=0;i<cap.size();++i) for (std::size_t j=i+1;j<cap.size();++j) {
        if (++export_boundary_work_>2097152) { strip_stop_="connector cap proof work budget";return false; }
        const bool adjacent=j==i+1 || (i==0 && j+1==cap.size());
        if (!certified_strip_pair(mesh->UVNode(cap[i]),mesh->UVNode(cap[(i+1)%cap.size()]),
            mesh->UVNode(cap[j]),mesh->UVNode(cap[(j+1)%cap.size()]),adjacent)) {
          strip_stop_="connector cap closure is not certified simple segments "+std::to_string(i)+'/'+std::to_string(j);return false;
        }
      }
      const double d=GetParameters().Deflection;
      const double angular=GetParameters().AngleInterior>0.0 ? GetParameters().AngleInterior : GetParameters().Angle;
      if (!std::isfinite(d) || d<=0.0 || !std::isfinite(angular) || angular<=0.0) { strip_stop_="connector cap precision budget";return false; }
      std::string cap_stop="no positive constrained ear";
      const auto precise=[&](const std::array<int,3>& cell) {
        gp_Pnt points[3];gp_Pnt2d uv[3];
        for (int i=0;i<3;++i) {
          points[i]=mesh->Node(cell[i]).Transformed(location.Transformation());uv[i]=mesh->UVNode(cell[i]);
          if (!strip_finite(points[i]) || !strip_finite(uv[i])) { cap_stop="nonfinite native cap ear";return false; }
        }
        const auto normal=gp_Vec(points[0],points[1]).Crossed(gp_Vec(points[0],points[2]));
        if (!std::isfinite(normal.SquareMagnitude()) || normal.SquareMagnitude()<=0.0) { cap_stop="zero native cap ear";return false; }
        const double weights[7][3]={{1,0,0},{0,1,0},{0,0,1},{.5,.5,0},{0,.5,.5},{.5,0,.5},{1.0/3,1.0/3,1.0/3}};
        int sample_index=0;
        for (const auto& w : weights) {
          if (++export_boundary_work_>2097152) { cap_stop="cap source work budget";return false; }
          const gp_Pnt2d sample(uv[0].Coord()*w[0]+uv[1].Coord()*w[1]+uv[2].Coord()*w[2]);
          const gp_Pnt affine(points[0].XYZ()*w[0]+points[1].XYZ()*w[1]+points[2].XYZ()*w[2]);
          gp_Pnt source;gp_Dir source_normal;
          if (!BRepMesh_GeomTool::Normal(trial.target->GetSurface(),sample.X(),sample.Y(),source,source_normal)) {
            cap_stop="cap source normal is undefined";return false;
          }
          const double error=affine.Distance(source),angle=normal.Angle(gp_Vec(source_normal));
          if (!strip_finite(source) || !strip_finite(affine) || !std::isfinite(error) || error>d || !std::isfinite(angle) || angle>angular) {
            std::ostringstream reason;reason.precision(8);
            reason << "ear " << cell[0] << '/' << cell[1] << '/' << cell[2] << " sample " << sample_index <<
                " source D/angle " << error << '/' << angle << " budgets " << d << '/' << angular;
            cap_stop=reason.str();return false;
          }
          ++sample_index;
        }
        return true;
      };
      std::vector<std::array<int,3>> cap_cells;int states=0;
      const auto search=[&](auto&& self,const std::vector<int>& polygon)->bool {
        if (++states>512 || ++export_boundary_work_>2097152) { cap_stop="cap constrained search/work budget";return false; }
        for (std::size_t i=0;i<polygon.size();++i) {
          const int previous=polygon[(i+polygon.size()-1)%polygon.size()],ear=polygon[i],next=polygon[(i+1)%polygon.size()];
          const auto a=mesh->UVNode(previous),b=mesh->UVNode(ear),c=mesh->UVNode(next);
          if (certified_strip_orientation(a,b,c)!=1) continue;
          bool valid=true;
          for (int id : polygon) {
            if (++export_boundary_work_>2097152) { cap_stop="cap vertex proof work budget";return false; }
            if (id==previous || id==ear || id==next) continue;
            const auto point=mesh->UVNode(id);
            // A certified negative half-plane proves the node lies outside.
            // All on/inside/unresolved points block this ear, including native
            // collinear stations that must remain constrained.
            if (certified_strip_orientation(a,b,point)!=-1 && certified_strip_orientation(b,c,point)!=-1 &&
                certified_strip_orientation(c,a,point)!=-1) { valid=false;break; }
          }
          for (std::size_t j=0;valid && j<polygon.size();++j) {
            if (++export_boundary_work_>2097152) { cap_stop="cap diagonal proof work budget";return false; }
            const int x=polygon[j],y=polygon[(j+1)%polygon.size()];
            if (x==previous || x==next || y==previous || y==next) continue;
            if (!certified_strip_pair(a,c,mesh->UVNode(x),mesh->UVNode(y),false)) valid=false;
          }
          const std::array<int,3> cell{previous,ear,next};
          if (!valid || !precise(cell)) continue;
          cap_cells.push_back(cell);
          if (polygon.size()==3) return true;
          auto remaining=polygon;remaining.erase(remaining.begin()+i);
          if (self(self,remaining)) return true;
          cap_cells.pop_back();
          if (states>=512 || export_boundary_work_>2097152) return false;
        }
        return false;
      };
      if (!search(search,cap) || cap_cells.size()!=cap.size()-2) {
        strip_stop_="connector constrained cap states "+std::to_string(states)+": "+cap_stop;return false;
      }
      for (const auto& cell : cap_cells) if (!append(cell)) { strip_stop_="certified cap installation failed";return false; }
      if (cells.size()!=boundary.size()-2) { strip_stop_="connector complete wire facet coverage";return false; }
      const auto replacement=mesh->Copy();replacement->ResizeTriangles(static_cast<int>(cells.size()),false);
      for (std::size_t i=0;i<cells.size();++i) replacement->SetTriangle(static_cast<int>(i)+1,Poly_Triangle(cells[i][0],cells[i][1],cells[i][2]));
      replacement->RemoveNormals();replacement->ComputeNormals();BRep_Builder().UpdateFace(trial.target->GetFace(),replacement);
      return true;
    } catch (const Standard_Failure&) { strip_stop_="OCCT exception qualifying connector zipper";return false; }
      catch (const std::exception&) { strip_stop_="exception qualifying connector zipper";return false; }
  }

  // Connectivity may ignore an exactly zero physical cell only when it
  // contains both UV representatives of the already proposed native pole.
  // Its full original chart image is still qualified after connectivity is
  // rebuilt; no positive-area triangle or unrelated zero cell is omitted.
  static bool certified_zero_pole_cell(const Handle(Poly_Triangulation)& mesh,
                                       const TopLoc_Location& location,const int* ids,
                                       const std::map<int,int>& aliases) {
    for (const auto& alias : aliases) {
      if (std::find(ids,ids+3,alias.first)==ids+3 || std::find(ids,ids+3,alias.second)==ids+3) continue;
      gp_Pnt points[3];
      for (int i=0;i<3;++i) {
        if (ids[i]<1 || ids[i]>mesh->NbNodes()) return false;
        points[i]=mesh->Node(ids[i]).Transformed(location.Transformation());
        if (!strip_finite(points[i])) return false;
      }
      const double area=gp_Vec(points[0],points[1]).Crossed(gp_Vec(points[0],points[2])).SquareMagnitude();
      return std::isfinite(area) && area==0.0;
    }
    return false;
  }

  void recover_export_boundaries() {
    int attempts=0,repaired=0,added=0;
    export_curve_diagnostics_=0;
    struct DeferredBoundary { StripTrial trial;int original;int choices; };
    std::vector<DeferredBoundary> deferred;
    std::size_t inspected=0;
    const auto& model=GetModel();
    TopTools_IndexedMapOfShape native_edges;
    TopExp::MapShapes(model->GetShape(),TopAbs_EDGE,native_edges);
    for (int fi=0;fi<model->FacesNb();++fi) {
      const auto& face=model->GetFace(fi);
      const int original=strip_original_faces_.FindIndex(face->GetFace())-1;
      if (original<0) throw std::runtime_error("Native export boundary face lacks original topology mapping");
      auto& why=export_boundary_rejections_[original];
      if ((face->GetStatusMask() & ~IMeshData_Outdated)!=0) { why="face failure or Reused prevents new polygon ownership"; continue; }
      TopLoc_Location location;
      const auto mesh=BRep_Tool::Triangulation(face->GetFace(),location);
      if (mesh.IsNull() || !mesh->HasUVNodes() || mesh->NbTriangles()<1 || mesh->NbTriangles()>131072 ||
          mesh->NbNodes()>65536) { why="no bounded UV triangulation"; continue; }
      if (face->WiresNb()!=1) { why="restoration requires one outer wire"; continue; }
      const auto wire=face->GetWire(0);
      if (wire->GetStatusMask()!=0) { why="wire status prevents restoration"; continue; }
      using Link=std::pair<int,int>;
      const auto link=[](int a,int b) { return std::make_pair(std::min(a,b),std::max(a,b)); };
      StripFace saved{face.get(),face->GetStatusMask(),{},mesh,{}};
      saved.original_orientation=strip_original_faces_.FindKey(original+1).Orientation();
      saved.wire_statuses.push_back(wire->GetStatusMask());
      std::map<int,int> pole_aliases;
      IMeshData::IEdgePtr pole_edge=nullptr;
      if (!qualify_native_pole(saved,mesh,location,pole_aliases,pole_edge,true,true)) {
        why="pole proposal: "+strip_stop_; continue;
      }
      const auto canonical=[&](int id) {
        const auto found=pole_aliases.find(id); return found==pole_aliases.end() ? id : found->second;
      };
      std::set<Link> expected; std::set<int> boundary_nodes;
      bool eligible=true;
      for (int ei=0;ei<wire->EdgesNb();++ei) {
        const auto edge=wire->GetEdge(ei);
        const auto orientation=wire->GetEdgeOrientation(ei);
        const auto pc=edge->GetPCurve(face.get(),orientation);
        const int pc_count=pc.IsNull() ? 0 : pc->ParametersNb();
        const int curve_count=edge->GetCurve()->ParametersNb();
        const bool native_degenerate=BRep_Tool::Degenerated(edge->GetEdge());
        const char* gate=(orientation!=TopAbs_FORWARD && orientation!=TopAbs_REVERSED) ? "orientation" :
            native_degenerate && !certified_pole_edge(edge,pc,pole_aliases) ? "native-degenerate" :
            edge->GetDegenerated() && !native_degenerate ? "discrete-degenerate" : pc.IsNull() ? "missing-pcurve" :
            pc_count<2 ? "too-few-samples" : pc_count>256 ? "sample-cap" : pc_count!=curve_count ? "sample-mismatch" : nullptr;
        if (gate) {
          TopoDS_Vertex first,last; TopExp::Vertices(edge->GetEdge(),first,last,false);
          std::ostringstream detail;
          detail << "edge " << native_edges.FindIndex(edge->GetEdge())-1 << " gate " << gate << " native/discrete-deg " <<
              native_degenerate << '/' << edge->GetDegenerated() << " orient " << static_cast<int>(orientation) <<
              " samples pc/3D " << pc_count << '/' << curve_count << " same-param/range " << edge->GetSameParam() << '/' <<
              edge->GetSameRange() << " endpoints ";
          if (first.IsNull() || last.IsNull()) detail << "missing";
          else detail << (first.IsSame(last) ? "same" : "distinct") << " XYZ-gap " << BRep_Tool::Pnt(first).Distance(BRep_Tool::Pnt(last));
          why=detail.str(); eligible=false; break;
        }
        if ((inspected+=pc->ParametersNb())>8000000) { why="export boundary inspection budget exhausted"; eligible=false; break; }
        std::vector<int> saved_indices;
        for (int i=0;i<pc->ParametersNb();++i) saved_indices.push_back(pc->GetIndex(i));
        saved.boundary_indices.emplace_back(pc,std::move(saved_indices));
        for (int i=0;i<pc->ParametersNb();++i) {
          const int id=pc->GetIndex(i);
          if (id<1 || id>mesh->NbNodes()) { why="native boundary index is unavailable"; eligible=false; break; }
          boundary_nodes.insert(canonical(id));
          if (i && canonical(pc->GetIndex(i-1))!=canonical(id)) expected.insert(link(canonical(pc->GetIndex(i-1)),canonical(id)));
        }
        if (!eligible) break;
      }
      if (!eligible) continue;
      if ((inspected+=3*static_cast<std::size_t>(mesh->NbTriangles()))>8000000) {
        why="export boundary inspection budget exhausted"; break;
      }
      std::map<Link,int> actual;
      int pole_zero_cells=0;
      for (int ti=1;ti<=mesh->NbTriangles();++ti) {
        int ids[3]; mesh->Triangle(ti).Get(ids[0],ids[1],ids[2]);
        for (int id : ids) if (id<1 || id>mesh->NbNodes()) { eligible=false;why="physical native incidence has invalid node";break; }
        if (!eligible) break;
        if (certified_zero_pole_cell(mesh,location,ids,pole_aliases)) { ++pole_zero_cells; continue; }
        gp_Pnt p[3];for (int i=0;i<3;++i) p[i]=mesh->Node(ids[i]).Transformed(location.Transformation());
        const double area=gp_Vec(p[0],p[1]).Crossed(gp_Vec(p[0],p[2])).SquareMagnitude();
        if (!strip_finite(p[0]) || !strip_finite(p[1]) || !strip_finite(p[2]) || !std::isfinite(area)) {
          eligible=false;why="physical native incidence has nonfinite facet";break;
        }
        // Detection must use the same positive physical facets as extraction.
        // This is no acceptance to omit their source UV regions: a complete
        // native-domain remesh must pass before the repaired face is retained.
        if (area==0.0) continue;
        for (int i=0;i<3;++i) ++actual[link(ids[i],ids[(i+1)%3])];
      }
      if (!eligible) continue;
      bool shortcut=false;
      for (const auto& entry : actual) if (entry.second==1 && !expected.count(entry.first)) {
        if (boundary_nodes.count(entry.first.first) && boundary_nodes.count(entry.first.second)) shortcut=true;
        else why="unexpected boundary has a node outside native wire mapping";
      }
      bool boundary_deficit=false;
      for (const auto& key : expected) if (actual.find(key)==actual.end()) boundary_deficit=true;
      if (!shortcut && !pole_zero_cells && !boundary_deficit) { if (why.empty()) why="no mapped-native boundary shortcut"; continue; }
      if (attempts>=128 || export_boundary_work_>=2097152) { why="export restoration attempt/work budget exhausted"; continue; }
      bool alias=false;
      for (int oi=1;oi<=strip_original_faces_.Extent();++oi) {
        const auto& other=strip_original_faces_.FindKey(oi);
        if (other.IsPartner(face->GetFace()) && !other.IsSame(face->GetFace())) { alias=true; break; }
      }
      if (alias) { why="located face alias requires shared polygon ownership"; continue; }
      StripTrial trial;
      trial.faces.push_back(saved);
      bool accepted=false;
      int unused_groups=0;
      for (const auto& pole : pole_aliases) {
        bool used=false;
        for (const auto& entry : actual) if (entry.second>0 && (entry.first.first==pole.first || entry.first.second==pole.first ||
            entry.first.first==pole.second || entry.first.second==pole.second)) { used=true;break; }
        if (!used) ++unused_groups;
      }
      std::string first_rejection;
      for (int choice=0;choice<(1<<unused_groups);++choice) {
        if (attempts>=128 || export_boundary_work_>=2097152) { why="export restoration attempt/work budget exhausted"; break; }
        ++attempts;
        try {
          restore_spherical_strip(trial);
          const bool restored=restore_skipped_strip_nodes(trial,true,choice);
          accepted=restored && validate_spherical_strip(trial,true);
          if (!accepted) why=std::string(restored ? "domain: " : "fan: ")+strip_stop_.substr(0,330);
          if (!accepted) {
            // A used skipped boundary node cannot be inserted into an isolated
            // ear without overlapping its existing facets. Rebuild the entire
            // certified native chart on the original disposable mesh instead.
            restore_spherical_strip(trial);
            const std::string local_rejection=why;
            const bool complete=restore_complete_export_face(trial,choice);
            accepted=complete && validate_spherical_strip(trial,true);
            if (!accepted) why="whole-face "+std::string(complete ? "domain: " : "star: ")+strip_stop_.substr(0,1300)+
                "; local "+local_rejection.substr(0,70);
          }
        } catch (const StripRollbackFailure&) { throw; }
          catch (const Standard_Failure&) { why="OCCT exception qualifying restored native boundary"; }
          catch (const std::exception&) { why="exception qualifying restored native boundary"; }
        if (!accepted) {
          restore_spherical_strip(trial);
          if (!choice) first_rejection=why;
          else why="unused-pole choices: 0 "+first_rejection.substr(0,650)+"; "+std::to_string(choice)+' '+why.substr(0,650);
          std::fprintf(stderr,"Native export boundary face %d choice %d policy original: %s\n",original,choice,why.substr(0,3000).c_str());
        } else {
          const auto replacement=BRep_Tool::Triangulation(face->GetFace(),location);
          ++repaired; added+=replacement->NbTriangles()-mesh->NbTriangles();
          why="native boundary restored and full domain certified"; break;
        }
      }
      if (!accepted) {
        deferred.push_back({trial,original,1<<unused_groups});
        const auto strip=strip_face_rejections_.find(original);
        if (strip!=strip_face_rejections_.end()) why="strip: "+strip->second.substr(0,350)+"; "+why;
      }
    }
    // Exhaust the previously qualified policy before spending any shared work
    // on an alternate. Each independent alternate starts from the original
    // mesh, index arrays and status masks, never a failed candidate's state.
    for (const auto& pending : deferred) {
      const auto& trial=pending.trial;const auto& saved=trial.faces.front();
      auto& why=export_boundary_rejections_[pending.original];
      const std::string original_reason=why;
      std::string alternate_reason;
      for (int choice=0;choice<pending.choices;++choice) {
        if (attempts>=128 || export_boundary_work_>=2097152) break;
        ++attempts;bool accepted=false;
        try {
          restore_spherical_strip(trial);
          const bool complete=restore_complete_export_face(trial,choice,true);
          accepted=complete && validate_spherical_strip(trial,true);
          alternate_reason=std::string(complete ? "domain: " : "star: ")+strip_stop_;
        } catch (const StripRollbackFailure&) { throw; }
          catch (const Standard_Failure&) { alternate_reason="OCCT exception qualifying lookahead native boundary"; }
          catch (const std::exception&) { alternate_reason="exception qualifying lookahead native boundary"; }
        if (accepted) {
          TopLoc_Location location;
          const auto mesh=BRep_Tool::Triangulation(saved.face->GetFace(),location);
          ++repaired;added+=mesh->NbTriangles()-saved.triangulation->NbTriangles();
          why="lookahead native boundary restored and full domain certified";break;
        }
        restore_spherical_strip(trial);
        std::fprintf(stderr,"Native export boundary face %d choice %d policy lookahead: %s\n",
            pending.original,choice,alternate_reason.substr(0,3000).c_str());
        why=original_reason.substr(0,1300)+"; lookahead choice "+std::to_string(choice)+' '+alternate_reason.substr(0,650);
      }
    }
    export_boundary_stop_="export boundary attempts/repaired/added triangles "+std::to_string(attempts)+"/"+
        std::to_string(repaired)+"/"+std::to_string(added)+"; continuous strip checks "+std::to_string(strip_continuous_checks_);
    export_boundary_attempts_=attempts;
  }

  // Refine a sampled crossing only when the corresponding exact, oriented
  // source PCurve intervals are disjoint. Snapshot after all earlier repairs;
  // neither a failed remesh nor rollback may undo their qualified geometry.
  // OCCT may register two distinct boundary stations as one UV mesh node.
  // Reconstruct the retained native stations and their authoritative trim
  // chains on disposable mesh copies, rather than conflating their identities
  // during export. A failed owner rolls back the entire shared-edge trial.
  void recover_export_merged_boundaries() {
    struct Station { IMeshData::IEdgePtr edge;IMeshData::IPCurveHandle pc;int sample; };
    int attempts=0,accepted=0;
    for (int fi=0;fi<GetModel()->FacesNb() && attempts<8 && export_boundary_attempts_<128 &&
         export_boundary_work_<2097152;++fi) {
      const auto face=GetModel()->GetFace(fi).get();
      // The observed merge is on a regular planar trim. Do not assume a
      // singular or periodic chart admits the same local cavity construction.
      if (face->GetSurface()->GetType()!=GeomAbs_Plane || face->WiresNb()!=1 ||
          (face->GetStatusMask() & ~IMeshData_Outdated)!=0) continue;
      const int original=strip_original_faces_.FindIndex(face->GetFace())-1;
      StripTrial trial;bool mutated=false,success=false;std::string reason;
      try {
        TopLoc_Location location;const auto mesh=BRep_Tool::Triangulation(face->GetFace(),location);
        if (mesh.IsNull() || !mesh->HasUVNodes() || mesh->NbNodes()>65536 || mesh->NbTriangles()>131072) continue;
        const auto wire=face->GetWire(0);std::map<int,std::vector<Station>> stations;
        for (int ei=0;ei<wire->EdgesNb();++ei) {
          const auto edge=wire->GetEdge(ei);const auto pc=edge->GetPCurve(face,wire->GetEdgeOrientation(ei));
          if (pc.IsNull() || pc->ParametersNb()<2 || pc->ParametersNb()>65536)
            throw std::runtime_error("merged boundary lacks bounded native stations");
          for (int i=0;i<pc->ParametersNb();++i) {
            if (++export_boundary_work_>2097152) throw std::runtime_error("merged boundary scan work cap");
            const int id=pc->GetIndex(i);
            if (id<1 || id>mesh->NbNodes()) throw std::runtime_error("merged boundary mapped index is invalid");
            stations[id].push_back({edge,pc,i});
          }
        }
        std::array<IMeshData::IEdgePtr,2> pair{nullptr,nullptr};
        for (const auto& group : stations) {
          if (group.second.size()!=2) continue;
          const auto& a=group.second[0];const auto& b=group.second[1];
          if (a.edge==b.edge || a.sample==0 || b.sample==0 || a.sample+1==a.pc->ParametersNb() ||
              b.sample+1==b.pc->ParametersNb()) continue;
          // Equal native stations need an identity proof, not this separation
          // repair. Here both stored coordinates and parameters remain fixed.
          if (!strip_finite(a.pc->GetPoint(a.sample)) || !strip_finite(b.pc->GetPoint(b.sample)) ||
              a.pc->GetPoint(a.sample).Distance(b.pc->GetPoint(b.sample))==0.0) continue;
          pair={a.edge,b.edge};break;
        }
        if (!pair[0]) continue;
        std::set<IMeshData::IFacePtr> owners;
        for (const auto edge : pair) {
          const auto curve=edge->GetCurve();
          if (!edge->GetSameParam() || !edge->GetSameRange() || edge->GetDegenerated() ||
              BRep_Tool::Degenerated(edge->GetEdge()) || edge->PCurvesNb()!=2 ||
              edge->GetPCurve(0)->GetFace()==edge->GetPCurve(1)->GetFace() ||
              curve->ParametersNb()<2 || curve->ParametersNb()>256)
            throw std::runtime_error("merged boundary native parameters/owners exceed certified scope");
          StripEdge saved{edge,edge->GetStatusMask(),{},{},{}};
          for (int i=0;i<curve->ParametersNb();++i) {
            const auto point=curve->GetPoint(i);const double parameter=curve->GetParameter(i);
            if (!strip_finite(point) || !std::isfinite(parameter) || (i && parameter<=curve->GetParameter(i-1)))
              throw std::runtime_error("merged boundary native station order/point is invalid");
            saved.points.push_back(point);saved.parameters.push_back(parameter);
          }
          for (int pi=0;pi<edge->PCurvesNb();++pi) {
            const auto pc=edge->GetPCurve(pi);const auto owner=pc->GetFace();owners.insert(owner);
            if (pc->ParametersNb()!=curve->ParametersNb()) throw std::runtime_error("merged boundary owner sample count mismatch");
            StripPCurve item{pc,{},{},{}};
            const double budget=std::min(GetParameters().Deflection/4.0,
                BRep_Tool::Tolerance(edge->GetEdge())+BRep_Tool::Tolerance(owner->GetFace()));
            for (int i=0;i<pc->ParametersNb();++i) {
              if (++export_boundary_work_>2097152) throw std::runtime_error("merged boundary source work cap");
              const auto uv=pc->GetPoint(i);const auto source=owner->GetSurface()->Value(uv.X(),uv.Y());
              if (pc->GetParameter(i)!=curve->GetParameter(i) || !strip_finite(uv) || !strip_finite(source) ||
                  !std::isfinite(budget) || budget<=0.0 || source.Distance(curve->GetPoint(i))>budget)
                throw std::runtime_error("merged boundary retained chart/native station exceeds tolerance or precision");
              item.points.push_back(uv);item.parameters.push_back(pc->GetParameter(i));item.indices.push_back(pc->GetIndex(i));
            }
            saved.pcurves.push_back(std::move(item));
          }
          trial.edges.push_back(std::move(saved));
        }
        if (owners.size()>4) throw std::runtime_error("merged boundary owner cap");
        for (const auto owner : owners) {
          if ((owner->GetStatusMask() & ~IMeshData_Outdated)!=0 || owner->WiresNb()!=1)
            throw std::runtime_error("merged boundary owner status or holes exceed local proof scope");
          for (int oi=1;oi<=strip_original_faces_.Extent();++oi) if (
              strip_original_faces_.FindKey(oi).IsPartner(owner->GetFace()) && !strip_original_faces_.FindKey(oi).IsSame(owner->GetFace()))
            throw std::runtime_error("merged boundary owner has an uncertified located alias");
          TopLoc_Location owner_location;const auto current=BRep_Tool::Triangulation(owner->GetFace(),owner_location);
          if (current.IsNull() || !current->HasUVNodes() || current->NbNodes()>65536 || current->NbTriangles()>131072)
            throw std::runtime_error("merged boundary owner has no bounded UV mesh");
          StripFace saved{owner,owner->GetStatusMask(),{},current,{}};
          const int key=strip_original_faces_.FindIndex(owner->GetFace());
          if (key<=0) throw std::runtime_error("merged boundary original owner mapping missing");
          saved.original_orientation=strip_original_faces_.FindKey(key).Orientation();
          const auto owner_wire=owner->GetWire(0);saved.wire_statuses.push_back(owner_wire->GetStatusMask());
          if (owner_wire->GetStatusMask()!=0) throw std::runtime_error("merged boundary owner wire status");
          std::size_t count=0;
          for (int ei=0;ei<owner_wire->EdgesNb();++ei) {
            const auto edge=owner_wire->GetEdge(ei);const auto pc=edge->GetPCurve(owner,owner_wire->GetEdgeOrientation(ei));
            if (BRep_Tool::Degenerated(edge->GetEdge()) || pc.IsNull() || (count+=pc->ParametersNb())>1024)
              throw std::runtime_error("merged boundary owner pole or station cap");
            std::vector<int> indices;
            for (int i=0;i<pc->ParametersNb();++i) indices.push_back(pc->GetIndex(i));
            saved.boundary_indices.push_back({pc,std::move(indices)});
          }
          trial.faces.push_back(std::move(saved));
        }
        ++attempts;++export_boundary_attempts_;mutated=true;
        if (!restore_strip_station_nodes(trial)) throw std::runtime_error(strip_stop_);
        if (!restore_skipped_strip_nodes(trial,true,0,true)) throw std::runtime_error("merged boundary native cavity: "+strip_stop_);
        for (const auto& saved : trial.faces) {
          // Separating the native cap station can expose a curved owner's
          // pre-existing angular error. Qualify its complete source chart on
          // independent mesh copies; never relax the requested precision.
          // This snapshot contains the NEW distinct boundary indices. The
          // outer trial alone owns restoration of their pre-separation state.
          TopLoc_Location current_location;
          StripFace current{saved.face,saved.face->GetStatusMask(),{},
              BRep_Tool::Triangulation(saved.face->GetFace(),current_location),{}};
          current.original_orientation=saved.original_orientation;
          for (int wi=0;wi<saved.face->WiresNb();++wi) {
            const auto wire=saved.face->GetWire(wi);current.wire_statuses.push_back(wire->GetStatusMask());
            for (int ei=0;ei<wire->EdgesNb();++ei) {
              const auto pc=wire->GetEdge(ei)->GetPCurve(saved.face,wire->GetEdgeOrientation(ei));
              if (pc.IsNull()) throw std::runtime_error("merged boundary owner retry lacks its current PCurve");
              std::vector<int> indices;
              for (int i=0;i<pc->ParametersNb();++i) indices.push_back(pc->GetIndex(i));
              current.boundary_indices.push_back({pc,std::move(indices)});
            }
          }
          StripTrial single;single.faces.push_back(std::move(current));
          bool qualified=validate_spherical_strip(single,true,true);
          std::string owner_stop=strip_stop_;
          for (int strategy=0;strategy<4 && !qualified;++strategy) {
            restore_spherical_strip(single);
            if (export_boundary_attempts_>=128 || export_boundary_work_>=2097152) {
              owner_stop="merged boundary owner refinement attempt/work cap";break;
            }
            ++export_boundary_attempts_;
            qualified=restore_complete_export_face(single,0,strategy>0,strategy>=2,strategy==3,false,true) &&
                validate_spherical_strip(single,true,true);
            owner_stop="owner "+std::to_string(strip_original_faces_.FindIndex(saved.face->GetFace())-1)+
                " strategy "+std::to_string(strategy)+" "+strip_stop_;
            if (!qualified) std::fprintf(stderr,"Native export merged boundary target %d %s\n",
                original+1,owner_stop.substr(0,3000).c_str());
          }
          if (!qualified) {
            diagnose_export_rails(saved.face);
            restore_spherical_strip(single);
            throw std::runtime_error("merged boundary owner complete source qualification: "+owner_stop);
          }
        }
        if (!validate_spherical_strip(trial,true,true)) throw std::runtime_error("merged boundary all-owner source/domain: "+strip_stop_);
        success=true;++accepted;
        export_boundary_rejections_[original]="separate native boundary stations and complete trim certified";
      } catch (const StripRollbackFailure&) { throw; }
        catch (const Standard_Failure&) { reason="OCCT exception reconstructing merged native boundary"; }
        catch (const std::exception& error) { reason=error.what(); }
      if (!success && mutated) restore_spherical_strip(trial);
      if (!success && !reason.empty()) {
        export_boundary_rejections_[original]+="; merged boundary: "+reason.substr(0,650);
        std::fprintf(stderr,"Native export merged boundary face %d rejected: %s\n",original+1,reason.substr(0,3000).c_str());
      }
    }
    export_boundary_stop_+="; merged boundary attempts/accepted "+std::to_string(attempts)+'/'+std::to_string(accepted);
  }

  // Read the final discrete constraints before ModelPostProcessor turns their
  // indices into native polygons. A shared mesh node is not evidence that two
  // distinct native edge samples are the same topological boundary point.
  // Keep the strict export identity rejection; this records the actual local
  // cavity needed to decide whether separate constraints can be reconstructed.
  void diagnose_export_boundary_merges() {
    try {
      TopTools_IndexedMapOfShape edges,vertices;
      TopExp::MapShapes(GetModel()->GetShape(),TopAbs_EDGE,edges);
      TopExp::MapShapes(GetModel()->GetShape(),TopAbs_VERTEX,vertices);
      struct Station {
        IMeshData::IEdgePtr edge;IMeshData::IPCurveHandle pc;
        int wire,occurrence,sample;std::array<int,3> key;
      };
      int work=0,reports=0;
      for (int fi=0;fi<GetModel()->FacesNb() && reports<4;++fi) {
        const auto face=GetModel()->GetFace(fi).get();TopLoc_Location location;
        const auto mesh=BRep_Tool::Triangulation(face->GetFace(),location);
        if (mesh.IsNull() || !mesh->HasUVNodes() || mesh->NbNodes()>65536 || mesh->NbTriangles()>131072) continue;
        std::map<int,std::vector<Station>> stations;
        for (int wi=0;wi<face->WiresNb();++wi) {
          const auto wire=face->GetWire(wi);
          for (int ei=0;ei<wire->EdgesNb();++ei) {
            const auto edge=wire->GetEdge(ei);const auto pc=edge->GetPCurve(face,wire->GetEdgeOrientation(ei));
            if (pc.IsNull() || pc->ParametersNb()<2 || pc->ParametersNb()>65536) continue;
            TopoDS_Vertex first,last;TopExp::Vertices(edge->GetEdge(),first,last,false);
            for (int i=0;i<pc->ParametersNb();++i) {
              if (++work>1048576) return;
              const int id=pc->GetIndex(i);if (id<1 || id>mesh->NbNodes()) continue;
              const bool endpoint=i==0 || i+1==pc->ParametersNb();
              const auto vertex=i==0 ? first : last;
              const std::array<int,3> key=endpoint ? std::array<int,3>{1,vertices.FindIndex(vertex),0} :
                  std::array<int,3>{2,edges.FindIndex(edge->GetEdge()),i};
              stations[id].push_back({edge,pc,wi,ei,i,key});
            }
          }
        }
        for (const auto& group : stations) {
          if (reports>=4) break;
          if (group.second.size()<2 || std::all_of(group.second.begin(),group.second.end(),
              [&](const Station& item) { return item.key==group.second.front().key; })) continue;
          ++reports;
          std::ostringstream detail;detail.precision(12);
          detail << "face " << strip_original_faces_.FindIndex(face->GetFace()) << " node " << group.first <<
              " type/wires/status " << static_cast<int>(face->GetSurface()->GetType()) << '/' << face->WiresNb() << '/' <<
              face->GetStatusMask() << " mesh nodes/triangles " << mesh->NbNodes() << '/' << mesh->NbTriangles();
          const auto point=mesh->Node(group.first).Transformed(location.Transformation());const auto uv=mesh->UVNode(group.first);
          detail << " mesh XYZ " << point.X() << '/' << point.Y() << '/' << point.Z() << " UV " << uv.X() << '/' << uv.Y();
          for (int wi=0;wi<face->WiresNb() && wi<8;++wi) {
            const auto wire=face->GetWire(wi);detail << "; wire " << wi << " edges/status " << wire->EdgesNb() << '/' << wire->GetStatusMask();
            for (int ei=0;ei<wire->EdgesNb() && ei<12;++ei) {
              const auto edge=wire->GetEdge(ei);const auto pc=edge->GetPCurve(face,wire->GetEdgeOrientation(ei));
              detail << " e" << edges.FindIndex(edge->GetEdge())-1 << ':' << static_cast<int>(wire->GetEdgeOrientation(ei)) <<
                  ':' << (pc.IsNull() ? 0 : pc->ParametersNb());
            }
          }
          int described=0;
          for (const auto& item : group.second) {
            if (++described>6) break;
            const auto curve=item.edge->GetCurve();
            detail << "; station wire/occurrence/kind/shape/sample " << item.wire << '/' << item.occurrence << '/' <<
                item.key[0] << '/' << item.key[1]-1 << '/' << item.sample << " native flags " << item.edge->GetSameParam() << '/' <<
                item.edge->GetSameRange() << " samples " << item.pc->ParametersNb() << '/' << curve->ParametersNb();
            if (item.sample>=curve->ParametersNb()) continue;
            const auto native=curve->GetPoint(item.sample);const auto chart=item.pc->GetPoint(item.sample);
            const double parameter=item.pc->GetParameter(item.sample);
            detail << " param " << parameter << " discrete XYZ " << native.X() << '/' << native.Y() << '/' << native.Z() <<
                " UV " << chart.X() << '/' << chart.Y() << " mesh gaps XYZ/UV " << native.Distance(point) << '/' << chart.Distance(uv) <<
                " edge/face tolerances " << BRep_Tool::Tolerance(item.edge->GetEdge()) << '/' << BRep_Tool::Tolerance(face->GetFace());
            double low,high;
            const auto source=BRep_Tool::CurveOnSurface(TopoDS::Edge(item.edge->GetEdge().Oriented(item.pc->GetOrientation())),face->GetFace(),low,high);
            if (!source.IsNull() && std::isfinite(parameter) && parameter>=low && parameter<=high) {
              const auto exact=source->Value(parameter);const auto surface=face->GetSurface()->Value(chart.X(),chart.Y());
              detail << " source UV " << exact.X() << '/' << exact.Y() << " source/discrete UV gap " << exact.Distance(chart) <<
                  " surface/native gap " << surface.Distance(native);
            }
            for (int i=std::max(0,item.sample-1);i<=std::min(item.pc->ParametersNb()-1,item.sample+1);++i) {
              const auto p=item.pc->GetPoint(i);
              detail << " neighbor " << i << ':' << item.pc->GetIndex(i) << ':' << p.X() << '/' << p.Y();
            }
          }
          int incidents=0;
          for (int ti=1;ti<=mesh->NbTriangles();++ti) {
            if (++work>1048576) break;
            int ids[3];mesh->Triangle(ti).Get(ids[0],ids[1],ids[2]);
            if (ids[0]!=group.first && ids[1]!=group.first && ids[2]!=group.first) continue;
            if (++incidents>16) continue;
            detail << "; incident " << ti << " nodes " << ids[0] << '/' << ids[1] << '/' << ids[2];
            for (int id : ids) {
              if (id<1 || id>mesh->NbNodes()) { detail << " invalid index";continue; }
              const auto p=mesh->UVNode(id);const auto x=mesh->Node(id).Transformed(location.Transformation());
              detail << " [" << p.X() << '/' << p.Y() << ";" << x.X() << '/' << x.Y() << '/' << x.Z() << ']';
            }
          }
          detail << "; total incident cells " << incidents;
          std::fprintf(stderr,"Native export merged boundary %s\n",detail.str().substr(0,7000).c_str());
        }
      }
    } catch (const Standard_Failure&) { std::fprintf(stderr,"Native export merged boundary diagnostic OCCT exception\n"); }
      catch (const std::exception&) { std::fprintf(stderr,"Native export merged boundary diagnostic exception\n"); }
  }

  void recover_export_chord_boundaries() {
    struct Segment { IMeshData::IEdgePtr edge;IMeshData::IPCurveHandle pc;int first,last; };
    int attempts=0,accepted=0,inserted=0;
    const auto crossing=[&](IMeshData::IFacePtr face,std::array<Segment,2>& pair,bool& simple) {
      simple=false;
      if (face->WiresNb()!=1) { strip_stop_="chord target requires one wire";return false; }
      const auto wire=face->GetWire(0);std::vector<Segment> segments;
      for (int ei=0;ei<wire->EdgesNb();++ei) {
        const auto edge=wire->GetEdge(ei);const auto orientation=wire->GetEdgeOrientation(ei);
        const auto pc=edge->GetPCurve(face,orientation);
        if (pc.IsNull() || pc->ParametersNb()<2 || pc->ParametersNb()>256 || BRep_Tool::Degenerated(edge->GetEdge()) ||
            (orientation!=TopAbs_FORWARD && orientation!=TopAbs_REVERSED)) {
          strip_stop_="chord target has unsupported native constraint";return false;
        }
        for (int i=1;i<pc->ParametersNb();++i) segments.push_back({edge,pc,
            orientation==TopAbs_REVERSED ? pc->ParametersNb()-i : i-1,
            orientation==TopAbs_REVERSED ? pc->ParametersNb()-i-1 : i});
      }
      if (segments.size()>1024) { strip_stop_="chord boundary segment cap";return false; }
      for (std::size_t i=0;i<segments.size();++i) for (std::size_t j=i+1;j<segments.size();++j) {
        if (++export_boundary_work_>2097152) { strip_stop_="chord crossing work cap";return false; }
        const bool adjacent=j==i+1 || (i==0 && j+1==segments.size());
        const auto a=segments[i].pc->GetPoint(segments[i].first),b=segments[i].pc->GetPoint(segments[i].last);
        const auto c=segments[j].pc->GetPoint(segments[j].first),d=segments[j].pc->GetPoint(segments[j].last);
        if (!strip_finite(a) || !strip_finite(b) || !strip_finite(c) || !strip_finite(d)) {
          strip_stop_="chord crossing has nonfinite source station";return false;
        }
        if (certified_strip_pair(a,b,c,d,adjacent)) continue;
        gp_Pnt2d hit;
        if (adjacent || segments[i].edge==segments[j].edge || BRepMesh_GeomTool::IntSegSeg(
              a.Coord(),b.Coord(),c.Coord(),d.Coord(),false,false,hit)!=BRepMesh_GeomTool::Cross) {
          strip_stop_="chord boundary has an uncertified contact or self-edge crossing";return false;
        }
        pair={segments[i],segments[j]};return true;
      }
      double area=0.0;std::string why;
      simple=simple_strip_boundary(face,area,true,&why);
      if (!simple) strip_stop_="chord refined boundary "+why;
      return false;
    };
    for (int fi=0;fi<GetModel()->FacesNb() && attempts<16 && export_boundary_attempts_<128;++fi) {
      const auto face=GetModel()->GetFace(fi).get();
      const int original=strip_original_faces_.FindIndex(face->GetFace())-1;
      const auto prior=export_boundary_rejections_.find(original);
      if (prior==export_boundary_rejections_.end() || prior->second.find("chart intersection")==std::string::npos ||
          (face->GetStatusMask() & ~IMeshData_Outdated)!=0 || export_boundary_work_>=2097152) continue;
      StripTrial trial;bool mutated=false,success=false;std::array<Segment,2> pair;bool simple=false;
      std::string reason;
      try {
        if (!crossing(face,pair,simple)) continue;
        std::set<IMeshData::IFacePtr> owners;
        for (const auto& segment : pair) {
          const auto edge=segment.edge;const auto curve=edge->GetCurve();
          if (!edge->GetSameParam() || !edge->GetSameRange() || edge->GetDegenerated() || edge->PCurvesNb()!=2 ||
              edge->GetPCurve(0)->GetFace()==edge->GetPCurve(1)->GetFace() || curve->ParametersNb()<2 || curve->ParametersNb()>248)
            throw std::runtime_error("chord native parameters or owners are not certified");
          StripEdge saved{edge,edge->GetStatusMask(),{},{},{}};
          for (int i=0;i<curve->ParametersNb();++i) {
            const auto point=curve->GetPoint(i);const double parameter=curve->GetParameter(i);
            if (!strip_finite(point) || !std::isfinite(parameter) || (i &&
                (parameter-curve->GetParameter(i-1))*(curve->GetParameter(1)-curve->GetParameter(0))<=0.0))
              throw std::runtime_error("chord native parameter order/point is invalid");
            saved.points.push_back(point);saved.parameters.push_back(parameter);
          }
          for (int pi=0;pi<edge->PCurvesNb();++pi) {
            const auto pc=edge->GetPCurve(pi);owners.insert(pc->GetFace());
            if (pc->ParametersNb()!=curve->ParametersNb()) throw std::runtime_error("chord owner sample count mismatch");
            StripPCurve item{pc,{},{},{}};
            for (int i=0;i<pc->ParametersNb();++i) {
              if (pc->GetParameter(i)!=curve->GetParameter(i) || !strip_finite(pc->GetPoint(i)))
                throw std::runtime_error("chord owner parameters/UV mismatch");
              item.points.push_back(pc->GetPoint(i));item.parameters.push_back(pc->GetParameter(i));item.indices.push_back(pc->GetIndex(i));
            }
            saved.pcurves.push_back(std::move(item));
          }
          trial.edges.push_back(std::move(saved));
        }
        if (owners.size()>4) throw std::runtime_error("chord owner cap");
        for (const auto owner : owners) {
          if ((owner->GetStatusMask() & ~IMeshData_Outdated)!=0 || owner->WiresNb()!=1) {
            std::ostringstream why;
            why << "chord owner " << strip_original_faces_.FindIndex(owner->GetFace())-1 << " status/wires/type/Uperiodic/Vperiodic " <<
                owner->GetStatusMask() << '/' << owner->WiresNb() << '/' << static_cast<int>(owner->GetSurface()->GetType()) << '/' <<
                owner->GetSurface()->IsUPeriodic() << '/' << owner->GetSurface()->IsVPeriodic();
            throw std::runtime_error(why.str());
          }
          for (int oi=1;oi<=strip_original_faces_.Extent();++oi) if (
              strip_original_faces_.FindKey(oi).IsPartner(owner->GetFace()) && !strip_original_faces_.FindKey(oi).IsSame(owner->GetFace()))
            throw std::runtime_error("chord owner has an uncertified located alias");
          TopLoc_Location location;const auto mesh=BRep_Tool::Triangulation(owner->GetFace(),location);
          if (mesh.IsNull() || !mesh->HasUVNodes() || mesh->NbNodes()>65536 || mesh->NbTriangles()>131072)
            throw std::runtime_error("chord owner mesh cap/UV");
          StripFace saved{owner,owner->GetStatusMask(),{},mesh,{}};
          const int key=strip_original_faces_.FindIndex(owner->GetFace());
          if (!key) throw std::runtime_error("chord owner original mapping missing");
          saved.original_orientation=strip_original_faces_.FindKey(key).Orientation();
          const auto wire=owner->GetWire(0);saved.wire_statuses.push_back(wire->GetStatusMask());
          if (wire->GetStatusMask()!=0) throw std::runtime_error("chord owner wire status");
          std::size_t count=0;
          for (int ei=0;ei<wire->EdgesNb();++ei) {
            const auto edge=wire->GetEdge(ei);const auto pc=edge->GetPCurve(owner,wire->GetEdgeOrientation(ei));
            if (BRep_Tool::Degenerated(edge->GetEdge()) || pc.IsNull() || (count+=pc->ParametersNb())>1024)
              throw std::runtime_error("chord owner pole/constraint cap");
            std::vector<int> indices;
            for (int i=0;i<pc->ParametersNb();++i) indices.push_back(pc->GetIndex(i));
            saved.boundary_indices.push_back({pc,std::move(indices)});
          }
          trial.faces.push_back(std::move(saved));
        }
        ++attempts;++export_boundary_attempts_;
        int added=0;
        for (int pass=0;pass<8;++pass) {
          Handle(Geom2d_Curve) intervals[2];
          for (int side=0;side<2;++side) {
            const auto& segment=pair[side];double low,high;
            const auto source=BRep_Tool::CurveOnSurface(TopoDS::Edge(segment.edge->GetEdge().Oriented(segment.pc->GetOrientation())),
                face->GetFace(),low,high);
            const double first=segment.pc->GetParameter(segment.first),last=segment.pc->GetParameter(segment.last);
            if (source.IsNull() || !std::isfinite(low) || !std::isfinite(high) || !std::isfinite(first) || !std::isfinite(last) ||
                first==last || std::min(first,last)<low || std::max(first,last)>high)
              throw std::runtime_error("chord source interval range is uncertified");
            intervals[side]=new Geom2d_TrimmedCurve(source,std::min(first,last),std::max(first,last),true,false);
          }
          Geom2dAPI_InterCurveCurve exact(intervals[0],intervals[1],Precision::PConfusion());
          if (!exact.Intersector().IsDone() || exact.Intersector().NbPoints()!=0 || exact.Intersector().NbSegments()!=0)
            throw std::runtime_error("chord continuous source intervals are not certified disjoint");
          for (const auto& segment : pair) {
            if (std::none_of(trial.edges.begin(),trial.edges.end(),[&](const StripEdge& saved) { return saved.edge==segment.edge; }))
              throw std::runtime_error("chord refinement reached an unrelated native edge");
            const auto curve=segment.edge->GetCurve();const int index=std::max(segment.first,segment.last);
            const double a=curve->GetParameter(index-1),b=curve->GetParameter(index),parameter=a+(b-a)*.5;
            if (!std::isfinite(parameter) || parameter==a || parameter==b || curve->ParametersNb()>=256)
              throw std::runtime_error("chord midpoint range/sample cap");
            const auto point=BRepAdaptor_Curve(segment.edge->GetEdge()).Value(parameter);
            if (!strip_finite(point)) throw std::runtime_error("chord midpoint native point is nonfinite");
            std::vector<gp_Pnt2d> uv;
            for (int pi=0;pi<segment.edge->PCurvesNb();++pi) {
              const auto pc=segment.edge->GetPCurve(pi);const auto owner=pc->GetFace();double low,high;
              const auto source=BRep_Tool::CurveOnSurface(TopoDS::Edge(segment.edge->GetEdge().Oriented(pc->GetOrientation())),owner->GetFace(),low,high);
              if (pc->ParametersNb()!=curve->ParametersNb() || pc->GetParameter(index-1)!=a || pc->GetParameter(index)!=b ||
                  source.IsNull() || !std::isfinite(low) || !std::isfinite(high) || std::min(a,b)<low || std::max(a,b)>high ||
                  ++export_boundary_work_>2097152) throw std::runtime_error("chord owner source parameter/work gate");
              auto sample=source->Value(parameter);
              const double budget=std::min(GetParameters().Deflection/4.0,
                  BRep_Tool::Tolerance(segment.edge->GetEdge())+BRep_Tool::Tolerance(owner->GetFace()));
              if (owner->GetSurface()->IsUPeriodic() || owner->GetSurface()->IsVPeriodic()) {
                const auto first=source->Value(a),last=source->Value(b);
                const auto old_first=pc->GetPoint(index-1),old_last=pc->GetPoint(index);
                if (!strip_finite(first) || !strip_finite(last) || !strip_finite(sample))
                  throw std::runtime_error("chord periodic source interval is nonfinite");
                const auto branch=[&](double start,double end,double middle,double saved_start,double saved_end,double period) {
                  if (!std::isfinite(period) || period<=0.0) throw std::runtime_error("chord surface period is invalid");
                  const double first_shift=std::round((saved_start-start)/period),last_shift=std::round((saved_end-end)/period);
                  if (!std::isfinite(first_shift) || first_shift!=last_shift || std::abs(first_shift)>1024.0)
                    throw std::runtime_error("chord periodic interval has inconsistent whole-period endpoints");
                  const double shift=first_shift*period,seed=(saved_start+saved_end)*.5;
                  const double margin=64.0*std::numeric_limits<double>::epsilon()*(std::abs(start)+std::abs(end)+
                      std::abs(middle)+std::abs(saved_start)+std::abs(saved_end)+std::abs(shift)+period);
                  if (!std::isfinite(margin) || std::abs(start+shift-saved_start)>=period*.5-margin ||
                      std::abs(end+shift-saved_end)>=period*.5-margin || std::abs(middle+shift-seed)>=period*.5-margin)
                    throw std::runtime_error("chord periodic branch is not uniquely local to saved interval");
                  return shift;
                };
                double u_shift=0.0,v_shift=0.0;
                if (owner->GetSurface()->IsUPeriodic()) u_shift=branch(first.X(),last.X(),sample.X(),old_first.X(),old_last.X(),owner->GetSurface()->UPeriod());
                if (owner->GetSurface()->IsVPeriodic()) v_shift=branch(first.Y(),last.Y(),sample.Y(),old_first.Y(),old_last.Y(),owner->GetSurface()->VPeriod());
                const gp_Pnt2d aligned_first(first.X()+u_shift,first.Y()+v_shift),aligned_last(last.X()+u_shift,last.Y()+v_shift);
                const auto endpoint_gap=[&](const gp_Pnt2d& exact,const gp_Pnt2d& saved) {
                  const auto a=owner->GetSurface()->Value(exact.X(),exact.Y()),b=owner->GetSurface()->Value(saved.X(),saved.Y());
                  return strip_finite(a) && strip_finite(b) && std::isfinite(budget) && budget>0.0 && a.Distance(b)<=budget;
                };
                if (!endpoint_gap(aligned_first,old_first) || !endpoint_gap(aligned_last,old_last))
                  throw std::runtime_error("chord periodic source endpoint exceeds recorded tolerance/precision");
                sample.SetCoord(sample.X()+u_shift,sample.Y()+v_shift);
              }
              const auto surface=owner->GetSurface()->Value(sample.X(),sample.Y());
              if (!strip_finite(sample) || !strip_finite(surface) || !std::isfinite(budget) || budget<=0.0 || point.Distance(surface)>budget)
                throw std::runtime_error("chord owner exact source/native sample exceeds tolerance or precision");
              uv.push_back(sample);
            }
            mutated=true;curve->InsertPoint(index,point,parameter);
            for (int pi=0;pi<segment.edge->PCurvesNb();++pi) segment.edge->GetPCurve(pi)->InsertPoint(index,uv[pi],parameter);
            ++added;
          }
          bool now_simple=false;
          if (!crossing(face,pair,now_simple)) {
            if (!now_simple) throw std::runtime_error(strip_stop_);
            break;
          }
          if (pass==7) throw std::runtime_error("chord refinement pass cap");
        }
        StripTrial synchronized;
        const bool paired=synchronize_export_source_rails(face,trial,synchronized,added);
        BRepMesh_MeshAlgoFactory factory;
        for (const auto& saved : trial.faces) {
          double area=0.0;std::string why;
          if (!simple_strip_boundary(saved.face,area,true,&why)) throw std::runtime_error("chord owner boundary remains nonsimple: "+why);
          const auto algo=factory.GetAlgo(saved.face->GetSurface()->GetType(),GetParameters());
          if (algo.IsNull()) throw std::runtime_error("chord owner mesher unavailable");
          BRep_Builder().UpdateFace(saved.face->GetFace(),Handle(Poly_Triangulation)());
          saved.face->SetStatus(IMeshData_Outdated);
          algo->Perform(saved.face,GetParameters(),Message_ProgressRange());
        }
        if (!restore_strip_station_nodes(trial)) throw std::runtime_error(strip_stop_);
        for (const auto& saved : trial.faces) {
          // These indices include the inserted stations. Independent owner
          // strategies restore this POST-insertion state; the outer transaction
          // alone restores its original pre-insertion snapshots on rejection.
          TopLoc_Location location;
          StripFace current{saved.face,saved.face->GetStatusMask(),{},BRep_Tool::Triangulation(saved.face->GetFace(),location),{}};
          current.original_orientation=saved.original_orientation;
          for (int wi=0;wi<saved.face->WiresNb();++wi) {
            const auto wire=saved.face->GetWire(wi);current.wire_statuses.push_back(wire->GetStatusMask());
            for (int ei=0;ei<wire->EdgesNb();++ei) {
              const auto pc=wire->GetEdge(ei)->GetPCurve(saved.face,wire->GetEdgeOrientation(ei));
              if (pc.IsNull()) throw std::runtime_error("chord owner retry lacks its inserted PCurve");
              std::vector<int> indices;
              for (int i=0;i<pc->ParametersNb();++i) indices.push_back(pc->GetIndex(i));
              current.boundary_indices.push_back({pc,std::move(indices)});
            }
          }
          StripTrial single;single.faces.push_back(std::move(current));
          bool qualified=paired && saved.face==face ?
              triangulate_connector_strip(synchronized) && validate_spherical_strip(single,true,true) :
              restore_skipped_strip_nodes(single,true) && validate_spherical_strip(single,true,true);
          std::string owner_stop=strip_stop_;
          for (int strategy=0;strategy<5 && !qualified;++strategy) {
            restore_spherical_strip(single);
            if (strategy>0) {
              if (export_boundary_attempts_>=128 || export_boundary_work_>=2097152) { owner_stop="chord owner alternate attempt/work cap";break; }
              ++export_boundary_attempts_;
            }
            const bool complete=strategy==0 ? restore_export_ear_face(single) :
                restore_complete_export_face(single,0,strategy>1,strategy>=3,strategy==4);
            qualified=complete && validate_spherical_strip(single,true,true);
            owner_stop="owner "+std::to_string(strip_original_faces_.FindIndex(saved.face->GetFace())-1)+
                " strategy "+std::to_string(strategy)+' '+strip_stop_;
            if (!qualified) std::fprintf(stderr,"Native export chord target %d %s\n",original,owner_stop.substr(0,3000).c_str());
          }
          if (!qualified) {
            diagnose_export_rails(saved.face);
            restore_spherical_strip(single);throw std::runtime_error("chord owner complete source qualification: "+owner_stop);
          }
        }
        if (!validate_spherical_strip(trial,true,true)) throw std::runtime_error("chord all-owner shared qualification: "+strip_stop_);
        success=true;++accepted;inserted+=added;
        export_boundary_rejections_[original]="continuous-source chord refinement certified all native owners";
      } catch (const StripRollbackFailure&) { throw; }
        catch (const Standard_Failure&) { reason="OCCT exception refining source chords"; }
        catch (const std::exception& error) { reason=error.what(); }
      if (!success && mutated) restore_spherical_strip(trial);
      if (!success && !reason.empty()) {
        export_boundary_rejections_[original]+="; chord: "+reason.substr(0,650);
        std::fprintf(stderr,"Native export chord face %d rejected: %s\n",original,reason.substr(0,3000).c_str());
      }
    }
    export_boundary_stop_+="; chord attempts/accepted/points "+std::to_string(attempts)+'/'+std::to_string(accepted)+'/'+std::to_string(inserted);
  }

  // Synchronize only a certified native three-edge chart: two strictly
  // monotone source rails share the same low-U native vertex, and their other
  // endpoints are joined by the untouched connector. This adds source samples
  // in the existing all-owner transaction; it never substitutes sphere bounds
  // for the generic chart's complete source/domain precision certificates.
  bool synchronize_export_source_rails(IMeshData::IFacePtr face,const StripTrial& trial,
                                      StripTrial& synchronized,int& added) {
    const auto skipped=[&](const char* reason) {
      std::fprintf(stderr,"Native export paired source rails face %d skipped: %s\n",
          strip_original_faces_.FindIndex(face->GetFace())-1,reason);return false;
    };
    if (face->WiresNb()!=1 || face->GetWire(0)->EdgesNb()!=3 || trial.edges.size()!=2) return skipped("native three-edge wire/two-rail gate");
    const auto wire=face->GetWire(0);
    std::array<IMeshData::IPCurveHandle,2> pcs;
    std::array<Handle(Geom2d_Curve),2> sources;
    std::array<TopoDS_Vertex,2> apex,outer;
    std::array<double,2> low_u,high_u;
    std::array<int,2> apex_indices;
    std::array<int,2> wire_indices{-1,-1};
    const auto uv_roundoff=[](const gp_Pnt2d& a,const gp_Pnt2d& b) {
      return 64.0*std::numeric_limits<double>::epsilon()*(std::abs(a.X())+std::abs(a.Y())+
          std::abs(b.X())+std::abs(b.Y())+1.0);
    };
    for (int side=0;side<2;++side) {
      const auto edge=trial.edges[side].edge;
      for (int ei=0;ei<3;++ei) if (wire->GetEdge(ei)==edge) wire_indices[side]=ei;
      if (wire_indices[side]<0) return skipped("shared rail is absent from native wire");
      pcs[side]=edge->GetPCurve(face,wire->GetEdgeOrientation(wire_indices[side]));
      const auto pc=pcs[side];
      if (pc.IsNull() || pc->ParametersNb()<2 || pc->ParametersNb()>128 ||
          pc->ParametersNb()!=edge->GetCurve()->ParametersNb()) return skipped("rail sample count/range gate");
      double first,last;
      sources[side]=BRep_Tool::CurveOnSurface(TopoDS::Edge(edge->GetEdge().Oriented(pc->GetOrientation())),face->GetFace(),first,last);
      BRepAdaptor_Curve adaptor(TopoDS::Edge(edge->GetEdge().Oriented(pc->GetOrientation())),face->GetFace());
      const auto curve=adaptor.CurveOnSurface().GetCurve();
      if (sources[side].IsNull() || sources[side]->IsPeriodic() || curve->GetType()!=GeomAbs_BSplineCurve) return skipped("source nonperiodic B-spline gate");
      const auto spline=curve->BSpline();
      if (spline.IsNull() || spline->IsRational() || spline->IsPeriodic() || spline->Degree()<1 || spline->Degree()>8 ||
          spline->NbPoles()<2 || spline->NbPoles()>128 || !std::isfinite(first) || !std::isfinite(last) || first>=last ||
          pc->GetParameter(0)<first || pc->GetParameter(pc->ParametersNb()-1)>last) return skipped("nonrational spline degree/poles/native range gate");
      const auto begin=sources[side]->Value(pc->GetParameter(0)),end=sources[side]->Value(pc->GetParameter(pc->ParametersNb()-1));
      if (!strip_finite(begin) || !strip_finite(end) || begin.X()==end.X()) return skipped("finite distinct source-U endpoints gate");
      const double direction=end.X()>begin.X() ? 1.0 : -1.0;
      // Every derivative control coefficient has this strict sign; positive
      // B-spline bases and valid knot denominators give a unique U root.
      for (int i=2;i<=spline->NbPoles();++i) {
        if (!strip_finite(spline->Pole(i)) || !strip_finite(spline->Pole(i-1)) ||
            (spline->Pole(i).X()-spline->Pole(i-1).X())*direction<=0.0) return skipped("strict signed control-U derivative gate");
      }
      for (int i=1;i<=spline->NbKnots();++i) if (!std::isfinite(spline->Knot(i)) ||
          (i>1 && spline->Knot(i)<=spline->Knot(i-1))) return skipped("finite ordered spline knot gate");
      for (int i=0;i<pc->ParametersNb();++i) {
        const auto exact=sources[side]->Value(pc->GetParameter(i));
        if (!strip_finite(exact) || exact.Distance(pc->GetPoint(i))>uv_roundoff(exact,pc->GetPoint(i)) ||
            (i && (pc->GetPoint(i).X()-pc->GetPoint(i-1).X())*direction<=0.0)) return skipped("retained source UV/strict station ordering gate");
      }
      TopoDS_Vertex native_first,native_last;TopExp::Vertices(edge->GetEdge(),native_first,native_last,false);
      if (native_first.IsNull() || native_last.IsNull()) return skipped("native rail endpoint identity gate");
      apex[side]=direction>0.0 ? native_first : native_last;
      outer[side]=direction>0.0 ? native_last : native_first;
      apex_indices[side]=direction>0.0 ? 0 : pc->ParametersNb()-1;
      low_u[side]=pc->GetPoint(apex_indices[side]).X();
      high_u[side]=pc->GetPoint(direction>0.0 ? pc->ParametersNb()-1 : 0).X();
    }
    if (!apex[0].IsSame(apex[1]) || outer[0].IsSame(outer[1])) return skipped("shared native apex/distinct outer endpoint identity gate");
    const auto first_apex=pcs[0]->GetPoint(apex_indices[0]),second_apex=pcs[1]->GetPoint(apex_indices[1]);
    if (first_apex.Distance(second_apex)>uv_roundoff(first_apex,second_apex)) return skipped("native apex source-chart coordinate roundoff gate");
    const auto native_apex=BRep_Tool::Pnt(apex[0]);
    for (int side=0;side<2;++side) {
      const auto edge=trial.edges[side].edge;
      const auto point=edge->GetCurve()->GetPoint(apex_indices[side]);
      const auto uv=pcs[side]->GetPoint(apex_indices[side]);
      const auto surface=face->GetSurface()->Value(uv.X(),uv.Y());
      const double budget=std::min(GetParameters().Deflection/4.0,BRep_Tool::Tolerance(edge->GetEdge())+BRep_Tool::Tolerance(face->GetFace()));
      if (!strip_finite(native_apex) || !strip_finite(point) || point.Distance(native_apex)!=0.0 ||
          !strip_finite(surface) || !std::isfinite(budget) || budget<=0.0 || surface.Distance(point)>budget)
        return skipped("native apex exact world identity/source precision gate");
    }
    const int connector=3-wire_indices[0]-wire_indices[1];
    TopoDS_Vertex connector_first,connector_last;TopExp::Vertices(wire->GetEdge(connector)->GetEdge(),connector_first,connector_last,false);
    if (connector_first.IsNull() || connector_last.IsNull() || !(
        (connector_first.IsSame(outer[0]) && connector_last.IsSame(outer[1])) ||
        (connector_first.IsSame(outer[1]) && connector_last.IsSame(outer[0])))) return skipped("native connector endpoint identity gate");
    const double common_end=std::min(high_u[0],high_u[1]);
    const double common_start=std::max(low_u[0],low_u[1]);
    std::set<double> stations;
    for (int side=0;side<2;++side) for (int i=0;i<pcs[side]->ParametersNb();++i) {
      if (i==apex_indices[side]) continue; // paired native endpoint, not a discarded station
      const double u=pcs[side]->GetPoint(i).X();
      if (u<=common_start) return skipped("non-endpoint station lies inside apex roundoff interval");
      if (u<=common_end) stations.insert(u);
    }
    if (stations.size()<2 || stations.size()>127 || *stations.rbegin()!=common_end) return skipped("retained common-U station budget/extent gate");
    for (int side=0;side<2;++side) for (double u : stations) {
      const auto edge=trial.edges[side].edge;const auto curve=edge->GetCurve();const auto pc=pcs[side];
      bool present=false;
      for (int i=0;i<pc->ParametersNb();++i) if (pc->GetPoint(i).X()==u) present=true;
      if (present) continue;
      int index=-1;
      for (int i=1;i<pc->ParametersNb();++i) if (u>std::min(pc->GetPoint(i-1).X(),pc->GetPoint(i).X()) &&
          u<std::max(pc->GetPoint(i-1).X(),pc->GetPoint(i).X())) { index=i;break; }
      if (index<1 || curve->ParametersNb()>=256) throw std::runtime_error("paired source rail station range/count");
      double a=curve->GetParameter(index-1),b=curve->GetParameter(index);
      const bool increasing=pc->GetPoint(index).X()>pc->GetPoint(index-1).X();
      for (int step=0;step<64;++step) {
        if (++export_boundary_work_>2097152) throw std::runtime_error("paired source rail inversion work cap");
        const double middle=a+(b-a)*.5;const auto at=sources[side]->Value(middle);
        if (!strip_finite(at)) throw std::runtime_error("paired source rail inversion is nonfinite");
        if ((at.X()<u)==increasing) a=middle;else b=middle;
      }
      const double parameter=a+(b-a)*.5;
      if (!std::isfinite(parameter) || parameter<=curve->GetParameter(index-1) || parameter>=curve->GetParameter(index))
        throw std::runtime_error("paired source rail root has no distinct native parameter");
      const auto point=BRepAdaptor_Curve(edge->GetEdge()).Value(parameter);
      auto target_uv=sources[side]->Value(parameter);const gp_Pnt2d matched(u,target_uv.Y());
      if (!strip_finite(point) || !strip_finite(target_uv) || target_uv.Distance(matched)>uv_roundoff(target_uv,matched))
        throw std::runtime_error("paired source rail root did not converge to coordinate roundoff");
      std::vector<gp_Pnt2d> values;
      for (int pi=0;pi<edge->PCurvesNb();++pi) {
        const auto owner_pc=edge->GetPCurve(pi);const auto owner=owner_pc->GetFace();double first,last;
        const auto source=BRep_Tool::CurveOnSurface(TopoDS::Edge(edge->GetEdge().Oriented(owner_pc->GetOrientation())),owner->GetFace(),first,last);
        if (source.IsNull() || owner_pc->ParametersNb()!=curve->ParametersNb() || !std::isfinite(first) || !std::isfinite(last) ||
            parameter<first || parameter>last || ++export_boundary_work_>2097152)
          throw std::runtime_error("paired source rail owner parameter/work gate");
        auto at=source->Value(parameter);const auto start=source->Value(curve->GetParameter(index-1)),end=source->Value(curve->GetParameter(index));
        if (!strip_finite(at) || !strip_finite(start) || !strip_finite(end)) throw std::runtime_error("paired source rail owner chart is nonfinite");
        const double budget=std::min(GetParameters().Deflection/4.0,BRep_Tool::Tolerance(edge->GetEdge())+BRep_Tool::Tolerance(owner->GetFace()));
        if (!std::isfinite(budget) || budget<=0.0) throw std::runtime_error("paired source rail owner precision budget is invalid");
        const auto branch=[&](double start,double end,double value,double old_start,double old_end,double period) {
          const double first_shift=std::round((old_start-start)/period),last_shift=std::round((old_end-end)/period);
          const double seed=(old_start+old_end)*.5;
          const double margin=64.0*std::numeric_limits<double>::epsilon()*(std::abs(start)+std::abs(end)+std::abs(value)+
              std::abs(old_start)+std::abs(old_end)+period+1.0);
          if (!std::isfinite(period) || period<=0.0 || !std::isfinite(first_shift) || first_shift!=last_shift || std::abs(first_shift)>1024.0 ||
              std::abs(start+first_shift*period-old_start)>=period*.5-margin ||
              std::abs(end+first_shift*period-old_end)>=period*.5-margin ||
              std::abs(value+first_shift*period-seed)>=period*.5-margin)
            throw std::runtime_error("paired source rail owner periodic branch is not unique");
          return first_shift*period;
        };
        double du=0.0,dv=0.0;
        if (owner->GetSurface()->IsUPeriodic()) du=branch(start.X(),end.X(),at.X(),owner_pc->GetPoint(index-1).X(),owner_pc->GetPoint(index).X(),owner->GetSurface()->UPeriod());
        if (owner->GetSurface()->IsVPeriodic()) dv=branch(start.Y(),end.Y(),at.Y(),owner_pc->GetPoint(index-1).Y(),owner_pc->GetPoint(index).Y(),owner->GetSurface()->VPeriod());
        for (int endpoint=0;endpoint<2;++endpoint) {
          const auto exact=endpoint ? end : start;const auto saved=owner_pc->GetPoint(index-1+endpoint);
          const auto source_point=owner->GetSurface()->Value(exact.X()+du,exact.Y()+dv);
          const auto saved_point=owner->GetSurface()->Value(saved.X(),saved.Y());
          if (!strip_finite(source_point) || !strip_finite(saved_point) || source_point.Distance(saved_point)>budget)
            throw std::runtime_error("paired source rail owner original chart branch exceeds precision");
        }
        at.SetCoord(at.X()+du,at.Y()+dv);
        if (owner_pc==pc) at=matched;
        const auto surface=owner->GetSurface()->Value(at.X(),at.Y());
        if (!strip_finite(at) || !strip_finite(surface) || !std::isfinite(budget) || budget<=0.0 || point.Distance(surface)>budget)
          throw std::runtime_error("paired source rail owner native/chart gap exceeds precision");
        values.push_back(at);
      }
      curve->InsertPoint(index,point,parameter);
      for (int pi=0;pi<edge->PCurvesNb();++pi) edge->GetPCurve(pi)->InsertPoint(index,values[pi],parameter);
      ++added;
    }
    synchronized.target=face;synchronized.target_edges=wire_indices;synchronized.connector=connector;
    synchronized.certified_native_apex=true;
    std::fprintf(stderr,"Native export paired source rails face %d common stations %zu total native stations %d/%d connector preserved\n",
        strip_original_faces_.FindIndex(face->GetFace())-1,stations.size(),pcs[0]->ParametersNb(),pcs[1]->ParametersNb());
    return true;
  }

  // A constrained boundary ear avoids artificial centroid spokes. It replaces
  // the complete mapped disk only after every positive ear passes the actual
  // source precision witnesses; no native station or collinear node is dropped.
  bool restore_export_ear_face(const StripTrial& trial) {
    strip_stop_="source ear preparation";
    try {
      if (trial.faces.size()!=1) return false;
      const auto face=trial.faces.front().face;TopLoc_Location location;
      const auto mesh=BRep_Tool::Triangulation(face->GetFace(),location);
      if (face->WiresNb()!=1 || mesh.IsNull() || !mesh->HasUVNodes() || (face->GetStatusMask() & ~IMeshData_Outdated)!=0) return false;
      std::vector<int> polygon;const auto wire=face->GetWire(0);
      for (int ei=0;ei<wire->EdgesNb();++ei) {
        const auto edge=wire->GetEdge(ei);const auto pc=edge->GetPCurve(face,wire->GetEdgeOrientation(ei));
        if (pc.IsNull() || BRep_Tool::Degenerated(edge->GetEdge()) || pc->ParametersNb()<2 || pc->ParametersNb()!=edge->GetCurve()->ParametersNb()) return false;
        for (int i=0;i<pc->ParametersNb();++i) {
          const int id=pc->GetIndex(i);
          if (id<1 || id>mesh->NbNodes() || !strip_finite(pc->GetPoint(i)) || !strip_finite(mesh->UVNode(id)) ||
              !strip_finite(mesh->Node(id)) || !strip_finite(edge->GetCurve()->GetPoint(i)) ||
              mesh->UVNode(id).Distance(pc->GetPoint(i))>Precision::PConfusion() ||
              mesh->Node(id).Transformed(location.Transformation()).Distance(edge->GetCurve()->GetPoint(i))>Precision::Confusion()) return false;
        }
        const auto next_edge=wire->GetEdge((ei+1)%wire->EdgesNb());
        const auto next_pc=next_edge->GetPCurve(face,wire->GetEdgeOrientation((ei+1)%wire->EdgesNb()));
        if (next_pc.IsNull() || next_pc->ParametersNb()<2) return false;
        const int end=wire->GetEdgeOrientation(ei)==TopAbs_REVERSED ? 0 : pc->ParametersNb()-1;
        const int start=wire->GetEdgeOrientation((ei+1)%wire->EdgesNb())==TopAbs_REVERSED ? next_pc->ParametersNb()-1 : 0;
        if (pc->GetIndex(end)!=next_pc->GetIndex(start)) { strip_stop_="source ear native junction identity differs";return false; }
        for (int i=0;i+1<pc->ParametersNb();++i) {
          const int index=wire->GetEdgeOrientation(ei)==TopAbs_REVERSED ? pc->ParametersNb()-1-i : i;
          polygon.push_back(pc->GetIndex(index));
          if (polygon.size()>32) { strip_stop_="source ear boundary cap";return false; }
        }
      }
      if (polygon.size()<3 || std::set<int>(polygon.begin(),polygon.end()).size()!=polygon.size()) return false;
      double area=0.0;
      const auto origin=mesh->UVNode(polygon.front());
      for (std::size_t i=0;i<polygon.size();++i) {
        const auto a=mesh->UVNode(polygon[i]),b=mesh->UVNode(polygon[(i+1)%polygon.size()]);
        if (!strip_finite(a) || !strip_finite(b) || a.Distance(b)==0.0) return false;
        area+=(a.Coord()-origin.Coord()).Crossed(b.Coord()-origin.Coord());
        for (std::size_t j=i+1;j<polygon.size();++j) {
          if (++export_boundary_work_>2097152) { strip_stop_="source ear simplicity cap";return false; }
          if (!certified_strip_pair(a,b,mesh->UVNode(polygon[j]),mesh->UVNode(polygon[(j+1)%polygon.size()]),
                j==i+1 || (i==0 && j+1==polygon.size()))) { strip_stop_="source ear mapped boundary is not simple";return false; }
        }
      }
      if (!std::isfinite(area) || area==0.0) return false;
      const double winding=area>0.0 ? 1.0 : -1.0;
      const double d=GetParameters().Deflection,angle=GetParameters().AngleInterior>0.0 ? GetParameters().AngleInterior : GetParameters().Angle;
      if (!std::isfinite(d) || !std::isfinite(angle) || d<=0.0 || angle<=0.0) return false;
      const double weights[7][3]={{1,0,0},{0,1,0},{0,0,1},{.5,.5,0},{0,.5,.5},{.5,0,.5},{1.0/3,1.0/3,1.0/3}};
      int states=0;std::vector<std::array<int,3>> triangles;
      const auto precise=[&](const std::array<int,3>& ids) {
        gp_Pnt p[3];gp_Pnt2d uv[3];
        for (int i=0;i<3;++i) {
          uv[i]=mesh->UVNode(ids[i]);p[i]=mesh->Node(ids[i]).Transformed(location.Transformation());
          if (!strip_finite(p[i]) || !strip_finite(uv[i])) return false;
        }
        if (winding*certified_strip_orientation(uv[0],uv[1],uv[2])<=0.0) return false;
        const auto normal=gp_Vec(p[0],p[1]).Crossed(gp_Vec(p[0],p[2]));
        if (!std::isfinite(normal.SquareMagnitude()) || normal.SquareMagnitude()<=0.0) return false;
        for (const auto& w : weights) {
          if (++export_boundary_work_>2097152) return false;
          const gp_Pnt2d at(uv[0].Coord()*w[0]+uv[1].Coord()*w[1]+uv[2].Coord()*w[2]);
          const gp_Pnt affine(p[0].XYZ()*w[0]+p[1].XYZ()*w[1]+p[2].XYZ()*w[2]);gp_Pnt source;gp_Dir source_normal;
          if (!BRepMesh_GeomTool::Normal(face->GetSurface(),at.X(),at.Y(),source,source_normal) || !strip_finite(source)) return false;
          const double error=affine.Distance(source),angular=normal.Angle(gp_Vec(source_normal)*winding);
          if (!std::isfinite(error) || !std::isfinite(angular) || error>d || angular>angle) return false;
        }
        return true;
      };
      const auto triangulate=[&](auto&& self,const std::vector<int>& active)->bool {
        if (++states>512 || ++export_boundary_work_>2097152) return false;
        if (active.size()==3) {
          const std::array<int,3> last={active[0],active[1],active[2]};
          if (!precise(last)) return false;triangles.push_back(last);return true;
        }
        for (std::size_t i=0;i<active.size();++i) {
          const auto prev=(i+active.size()-1)%active.size(),next=(i+1)%active.size();
          const std::array<int,3> ear={active[prev],active[i],active[next]};
          const auto a=mesh->UVNode(ear[0]),b=mesh->UVNode(ear[1]),c=mesh->UVNode(ear[2]);
          if (winding*certified_strip_orientation(a,b,c)<=0.0) continue;
          bool clear=true;
          for (std::size_t j=0;j<active.size() && clear;++j) {
            if (++export_boundary_work_>2097152) return false;
            if (j!=prev && j!=i && j!=next) {
              const auto p=mesh->UVNode(active[j]);
              clear=winding*certified_strip_orientation(a,b,p)<0.0 || winding*certified_strip_orientation(b,c,p)<0.0 ||
                  winding*certified_strip_orientation(c,a,p)<0.0;
            }
            const auto end=(j+1)%active.size();
            if (clear && j!=prev && j!=next && end!=prev && end!=next)
              clear=certified_strip_pair(a,c,mesh->UVNode(active[j]),mesh->UVNode(active[end]),false);
          }
          if (!clear || !precise(ear)) continue;
          auto remainder=active;remainder.erase(remainder.begin()+i);triangles.push_back(ear);
          if (self(self,remainder)) return true;
          triangles.pop_back();
          if (states>=512 || export_boundary_work_>2097152) return false;
        }
        return false;
      };
      if (!triangulate(triangulate,polygon) || triangles.size()+2!=polygon.size()) {
        strip_stop_="source ear no fully precise constrained triangulation; states "+std::to_string(states);return false;
      }
      const auto replacement=mesh->Copy();replacement->ResizeTriangles(static_cast<int>(triangles.size()),false);
      for (std::size_t i=0;i<triangles.size();++i) replacement->SetTriangle(static_cast<int>(i)+1,Poly_Triangle(triangles[i][0],triangles[i][1],triangles[i][2]));
      replacement->RemoveNormals();replacement->ComputeNormals();BRep_Builder().UpdateFace(face->GetFace(),replacement);
      strip_stop_="fully source-qualified constrained ears installed";return true;
    } catch (const Standard_Failure&) { strip_stop_="OCCT exception certifying source ears";return false; }
      catch (const std::exception&) { strip_stop_="exception certifying source ears";return false; }
  }

  void diagnose_export_rails(IMeshData::IFacePtr face) const {
    try {
      if (face->WiresNb()!=1 || face->GetWire(0)->EdgesNb()>8) return;
      TopTools_IndexedMapOfShape native_edges,vertices;
      TopExp::MapShapes(GetModel()->GetShape(),TopAbs_EDGE,native_edges);
      TopExp::MapShapes(GetModel()->GetShape(),TopAbs_VERTEX,vertices);
      const int original=strip_original_faces_.FindIndex(face->GetFace())-1;
      double u0,u1,v0,v1;BRepTools::UVBounds(face->GetFace(),u0,u1,v0,v1);
      std::fprintf(stderr,"Native export source chart face %d surface-type %d wire-edges %d U/V %.12g/%.12g:%.12g/%.12g\n",
          original,static_cast<int>(face->GetSurface()->GetType()),face->GetWire(0)->EdgesNb(),u0,u1,v0,v1);
      if (face->GetSurface()->GetType()==GeomAbs_BSplineSurface) {
        const auto source=face->GetSurface()->BSpline();
        if (!source.IsNull()) std::fprintf(stderr,"Native export source chart face %d U/V degree %d/%d poles %d/%d rational %d/%d\n",
            original,source->UDegree(),source->VDegree(),source->NbUPoles(),source->NbVPoles(),source->IsURational(),source->IsVRational());
      }
      struct Rail { double span;IMeshData::IPCurveHandle pc;Handle(Geom2d_Curve) source;int edge; };
      std::vector<Rail> rails;const auto wire=face->GetWire(0);
      for (int ei=0;ei<wire->EdgesNb();++ei) {
        const auto edge=wire->GetEdge(ei);const auto pc=edge->GetPCurve(face,wire->GetEdgeOrientation(ei));
        if (pc.IsNull() || pc->ParametersNb()<2 || pc->ParametersNb()>64) continue;
        const int count=pc->ParametersNb(),key=native_edges.FindIndex(edge->GetEdge())-1;
        double low,high;const auto source=BRep_Tool::CurveOnSurface(TopoDS::Edge(edge->GetEdge().Oriented(pc->GetOrientation())),face->GetFace(),low,high);
        if (source.IsNull()) continue;
        double u0=std::numeric_limits<double>::infinity(),u1=-u0,v0=u0,v1=-u0;
        const double direction=pc->GetPoint(count-1).X()-pc->GetPoint(0).X();bool monotone=direction!=0.0;
        for (int i=0;i<count;++i) {
          const auto uv=pc->GetPoint(i);if (!strip_finite(uv)) return;
          u0=std::min(u0,uv.X());u1=std::max(u1,uv.X());v0=std::min(v0,uv.Y());v1=std::max(v1,uv.Y());
          if (i) monotone &= (uv.X()-pc->GetPoint(i-1).X())*direction>0.0;
        }
        TopoDS_Vertex first,last;TopExp::Vertices(edge->GetEdge(),first,last,false);
        BRepAdaptor_Curve adaptor(TopoDS::Edge(edge->GetEdge().Oriented(pc->GetOrientation())),face->GetFace());
        const auto curve=adaptor.CurveOnSurface().GetCurve();
        std::ostringstream detail;detail.precision(12);
        detail << "Native export rail catalog face " << original << " wire-edge/native " << ei << '/' << key <<
            " orientation " << static_cast<int>(pc->GetOrientation()) << " native-param vertices " << vertices.FindIndex(first)-1 << '/' << vertices.FindIndex(last)-1 <<
            " native/discrete deg " << BRep_Tool::Degenerated(edge->GetEdge()) << '/' << edge->GetDegenerated() <<
            " samples " << count << " params " << pc->GetParameter(0) << '/' << pc->GetParameter(count-1) <<
            " U/Vbounds " << u0 << '/' << u1 << ':' << v0 << '/' << v1 << " sampled-Umonotone " << monotone <<
            " source-type/periodic " << static_cast<int>(curve->GetType()) << '/' << source->IsPeriodic();
        if (curve->GetType()==GeomAbs_BSplineCurve) {
          const auto spline=curve->BSpline();bool poles_monotone=true;
          if (!spline.IsNull() && spline->NbPoles()<=128) {
            for (int i=2;i<=spline->NbPoles();++i) poles_monotone &= (spline->Pole(i).X()-spline->Pole(i-1).X())*direction>=0.0;
            detail << " degree/poles/rational/control-Umonotone " << spline->Degree() << '/' << spline->NbPoles() << '/' << spline->IsRational() << '/' << poles_monotone;
          }
        }
        std::set<int> anchors={0,1,count/2,count-2,count-1};
        for (int i : anchors) {
          if (i<0 || i>=count || i>=edge->GetCurve()->ParametersNb()) continue;
          const auto uv=pc->GetPoint(i),exact=source->Value(pc->GetParameter(i));
          const auto native=edge->GetCurve()->GetPoint(i);gp_Pnt surface;gp_Dir normal;
          if (!strip_finite(exact) || !strip_finite(native) || !BRepMesh_GeomTool::Normal(face->GetSurface(),uv.X(),uv.Y(),surface,normal) || !strip_finite(surface)) continue;
          detail << " anchor " << i << " t/UV " << pc->GetParameter(i) << ':' << uv.X() << '/' << uv.Y() <<
              " gap/normal-component " << native.Distance(surface) << '/' << std::abs(gp_Vec(surface,native).Dot(gp_Vec(normal))) <<
              " sourceUV " << exact.X() << '/' << exact.Y();
        }
        std::fprintf(stderr,"%s\n",detail.str().substr(0,4000).c_str());
        if (monotone) rails.push_back({u1-u0,pc,source,key});
      }
      std::sort(rails.begin(),rails.end(),[](const Rail& a,const Rail& b) { return a.span>b.span; });
      if (rails.size()<2 || rails[0].edge==rails[1].edge) return;
      const auto range=[](const Rail& rail) {
        const auto a=rail.source->Value(rail.pc->GetParameter(0)),b=rail.source->Value(rail.pc->GetParameter(rail.pc->ParametersNb()-1));
        return std::make_pair(std::min(a.X(),b.X()),std::max(a.X(),b.X()));
      };
      const auto first=range(rails[0]),second=range(rails[1]);const double start=std::max(first.first,second.first),end=std::min(first.second,second.second);
      if (!std::isfinite(start) || !std::isfinite(end) || start>=end) return;
      for (double fraction : {.25,.5,.75}) {
        const double u=start+(end-start)*fraction;gp_Pnt2d uv[2];gp_Pnt points[2];
        for (int side=0;side<2;++side) {
          const auto& rail=rails[side];double a=rail.pc->GetParameter(0),b=rail.pc->GetParameter(rail.pc->ParametersNb()-1);
          const bool ascending=rail.source->Value(b).X()>rail.source->Value(a).X();
          for (int step=0;step<48;++step) {
            const double middle=a+(b-a)*.5;uv[side]=rail.source->Value(middle);
            if (!strip_finite(uv[side])) return;
            if ((uv[side].X()<u)==ascending) a=middle;else b=middle;
          }
          points[side]=face->GetSurface()->Value(uv[side].X(),uv[side].Y());if (!strip_finite(points[side])) return;
        }
        std::fprintf(stderr,"Native export rail width face %d edges %d/%d sampled-root U %.12g UV-V %.12g/%.12g source-width-mm %.12g\n",
            original,rails[0].edge,rails[1].edge,u,uv[0].Y(),uv[1].Y(),points[0].Distance(points[1]));
      }
    } catch (const Standard_Failure&) { std::fprintf(stderr,"Native export rail diagnostic unavailable (OCCT)\n"); }
      catch (const std::exception&) { std::fprintf(stderr,"Native export rail diagnostic unavailable (native)\n"); }
  }

  // Try geometric span contraction only after every earlier strategy and the
  // shared-curve transaction. Fresh snapshots protect all accepted repairs.
  void recover_export_longest_faces(bool allow_flips=false,bool boundary_ears=false) {
    int attempts=0,repaired=0,added=0;
    for (int fi=0;fi<GetModel()->FacesNb() && export_boundary_attempts_<128 && export_boundary_work_<2097152;++fi) {
      const auto face=GetModel()->GetFace(fi).get();
      const int original=strip_original_faces_.FindIndex(face->GetFace())-1;
      const auto rejection=export_boundary_rejections_.find(original);
      if (rejection==export_boundary_rejections_.end() || rejection->second.find("whole-face")==std::string::npos ||
          rejection->second.find("depth")==std::string::npos || face->WiresNb()!=1 || (face->GetStatusMask() & ~IMeshData_Outdated)!=0) continue;
      StripTrial trial;std::string reason;int choices=1;
      try {
        TopLoc_Location location;const auto mesh=BRep_Tool::Triangulation(face->GetFace(),location);
        if (mesh.IsNull() || !mesh->HasUVNodes() || mesh->NbNodes()>65536 || mesh->NbTriangles()>131072) continue;
        bool located_alias=false;
        for (int oi=1;oi<=strip_original_faces_.Extent();++oi) if (
            strip_original_faces_.FindKey(oi).IsPartner(face->GetFace()) && !strip_original_faces_.FindKey(oi).IsSame(face->GetFace())) {
          located_alias=true;break;
        }
        if (located_alias) continue;
        StripFace saved{face,face->GetStatusMask(),{},mesh,{}};
        if (original<0) throw std::runtime_error("longest-face original topology mapping missing");
        saved.original_orientation=strip_original_faces_.FindKey(original+1).Orientation();
        const auto wire=face->GetWire(0);saved.wire_statuses.push_back(wire->GetStatusMask());
        if (wire->GetStatusMask()!=0) continue;
        std::size_t samples=0;
        for (int ei=0;ei<wire->EdgesNb();++ei) {
          const auto pc=wire->GetEdge(ei)->GetPCurve(face,wire->GetEdgeOrientation(ei));
          if (pc.IsNull() || (samples+=pc->ParametersNb())>1024) throw std::runtime_error("longest-face snapshot sample cap");
          std::vector<int> indices;
          for (int i=0;i<pc->ParametersNb();++i) indices.push_back(pc->GetIndex(i));
          saved.boundary_indices.push_back({pc,std::move(indices)});
        }
        std::map<int,int> aliases;IMeshData::IEdgePtr pole=nullptr;
        if (!qualify_native_pole(saved,mesh,location,aliases,pole,true,true)) continue;
        std::set<int> used;
        for (int ti=1;ti<=mesh->NbTriangles();++ti) {
          if (++export_boundary_work_>2097152) throw std::runtime_error("longest-face physical incidence budget");
          int ids[3];mesh->Triangle(ti).Get(ids[0],ids[1],ids[2]);gp_Pnt p[3];
          for (int i=0;i<3;++i) {
            if (ids[i]<1 || ids[i]>mesh->NbNodes()) throw std::runtime_error("longest-face invalid original facet");
            p[i]=mesh->Node(ids[i]).Transformed(location.Transformation());
            if (!strip_finite(p[i])) throw std::runtime_error("longest-face nonfinite original facet");
          }
          const double area=gp_Vec(p[0],p[1]).Crossed(gp_Vec(p[0],p[2])).SquareMagnitude();
          if (!std::isfinite(area)) throw std::runtime_error("longest-face nonfinite original area");
          if (area>0.0) for (int id : ids) used.insert(id);
        }
        int unused=0;
        for (const auto& alias : aliases) if (!used.count(alias.first) && !used.count(alias.second)) ++unused;
        if (unused>2) throw std::runtime_error("longest-face unused pole choice cap");
        choices=1<<unused;trial.faces.push_back(std::move(saved));
      } catch (const Standard_Failure&) { continue; }
        catch (const std::exception&) { continue; }
      const std::string prior=rejection->second;
      for (int choice=0;choice<choices && export_boundary_attempts_<128 && export_boundary_work_<2097152;++choice) {
        ++attempts;++export_boundary_attempts_;bool accepted=false;
        try {
          restore_spherical_strip(trial);
          if (boundary_ears && choice==0) diagnose_export_rails(face);
          const bool complete=restore_complete_export_face(trial,choice,true,true,allow_flips,boundary_ears);
          accepted=complete && validate_spherical_strip(trial,true);
          reason=std::string(complete ? "domain: " : "star: ")+strip_stop_;
        } catch (const StripRollbackFailure&) { throw; }
          catch (const Standard_Failure&) { reason="OCCT exception qualifying longest-face alternate"; }
          catch (const std::exception&) { reason="exception qualifying longest-face alternate"; }
        if (accepted) {
          TopLoc_Location location;const auto mesh=BRep_Tool::Triangulation(face->GetFace(),location);
          ++repaired;added+=mesh->NbTriangles()-trial.faces.front().triangulation->NbTriangles();
          rejection->second=std::string(boundary_ears ? "boundary ears" : allow_flips ? "deferred flip" : "longest-edge")+" native boundary restored and full domain certified";break;
        }
        restore_spherical_strip(trial);
        rejection->second=prior.substr(0,1300)+(allow_flips ? "; deferred flip choice " : "; longest choice ")+std::to_string(choice)+' '+reason.substr(0,650);
        std::fprintf(stderr,"Native export boundary face %d choice %d policy %s: %s\n",original,choice,
            boundary_ears ? "boundary-ears" : allow_flips ? "deferred-flip" : "longest",reason.substr(0,3000).c_str());
      }
    }
    export_boundary_stop_+=std::string(boundary_ears ? "; boundary ears" : allow_flips ? "; deferred flip" : "; longest")+" attempts/repaired/added "+std::to_string(attempts)+'/'+std::to_string(repaired)+'/'+std::to_string(added);
  }

  // This is only a different initial triangulation of the same certified disk,
  // not a precision acceptance. Quality orders ears by actual source witnesses;
  // every resulting cell still enters the unchanged conformal refinement and
  // must satisfy all seven source witnesses before installation.
  bool build_export_boundary_ears(IMeshData::IFacePtr face,const Handle(Poly_Triangulation)& mesh,
                                 const TopLoc_Location& location,const std::vector<int>& boundary,
                                 double winding,double d,double angle,std::vector<std::array<int,3>>& seed) {
    if (boundary.size()<3 || boundary.size()>256 || !std::isfinite(d) || d<=0.0 || !std::isfinite(angle) || angle<=0.0) {
      strip_stop_="boundary-ear source/count budget";return false;
    }
    const double weights[7][3]={{1,0,0},{0,1,0},{0,0,1},{.5,.5,0},{0,.5,.5},{.5,0,.5},{1.0/3,1.0/3,1.0/3}};
    const auto quality=[&](const std::array<int,3>& ids,double& score) {
      gp_Pnt2d uv[3];gp_Pnt p[3];
      for (int i=0;i<3;++i) {
        uv[i]=mesh->UVNode(ids[i]);p[i]=mesh->Node(ids[i]).Transformed(location.Transformation());
        if (!strip_finite(uv[i]) || !strip_finite(p[i])) return false;
      }
      const auto normal=gp_Vec(p[0],p[1]).Crossed(gp_Vec(p[0],p[2]));
      if (!std::isfinite(normal.SquareMagnitude()) || normal.SquareMagnitude()<=0.0) return false;
      score=0.0;
      for (const auto& w : weights) {
        if (++export_boundary_work_>2097152) return false;
        const gp_Pnt2d at(uv[0].Coord()*w[0]+uv[1].Coord()*w[1]+uv[2].Coord()*w[2]);
        const gp_Pnt affine(p[0].XYZ()*w[0]+p[1].XYZ()*w[1]+p[2].XYZ()*w[2]);gp_Pnt source;gp_Dir source_normal;
        if (!BRepMesh_GeomTool::Normal(face->GetSurface(),at.X(),at.Y(),source,source_normal) || !strip_finite(source)) return false;
        const double error=affine.Distance(source),angular=normal.Angle(gp_Vec(source_normal)*winding);
        if (!std::isfinite(error) || !std::isfinite(angular)) return false;
        score=std::max(score,std::max(error/d,angular/angle));
      }
      return std::isfinite(score);
    };
    struct Ear { std::size_t index;std::array<int,3> ids;double score,aspect; };
    int states=0;
    const auto search=[&](auto&& self,const std::vector<int>& polygon)->bool {
      if (++states>512 || ++export_boundary_work_>2097152) return false;
      std::vector<Ear> ears;
      for (std::size_t i=0;i<polygon.size();++i) {
        const auto previous=(i+polygon.size()-1)%polygon.size(),next=(i+1)%polygon.size();
        const std::array<int,3> ids{polygon[previous],polygon[i],polygon[next]};
        const auto a=mesh->UVNode(ids[0]),b=mesh->UVNode(ids[1]),c=mesh->UVNode(ids[2]);
        if (winding*certified_strip_orientation(a,b,c)<=0.0) continue;
        bool valid=true;
        for (std::size_t j=0;j<polygon.size() && valid;++j) {
          if (++export_boundary_work_>2097152) return false;
          if (j!=previous && j!=i && j!=next) {
            const auto p=mesh->UVNode(polygon[j]);
            valid=winding*certified_strip_orientation(a,b,p)<0.0 || winding*certified_strip_orientation(b,c,p)<0.0 ||
                winding*certified_strip_orientation(c,a,p)<0.0;
          }
          const auto end=(j+1)%polygon.size();
          if (valid && j!=previous && j!=next && end!=previous && end!=next)
            valid=certified_strip_pair(a,c,mesh->UVNode(polygon[j]),mesh->UVNode(polygon[end]),false);
        }
        double score;
        if (!valid || !quality(ids,score)) continue;
        const double area=std::abs((b.Coord()-a.Coord()).Crossed(c.Coord()-a.Coord()));
        const double length2=a.SquareDistance(b)+b.SquareDistance(c)+c.SquareDistance(a);
        const double aspect=area/length2;
        if (!std::isfinite(aspect) || aspect<=0.0) continue;
        ears.push_back({i,ids,score,aspect});
      }
      std::stable_sort(ears.begin(),ears.end(),[](const Ear& a,const Ear& b) {
        return a.score!=b.score ? a.score<b.score : a.aspect>b.aspect;
      });
      for (const auto& ear : ears) {
        seed.push_back(ear.ids);
        if (polygon.size()==3) return true;
        auto remainder=polygon;remainder.erase(remainder.begin()+ear.index);
        if (self(self,remainder)) return true;
        seed.pop_back();
        if (states>=512 || export_boundary_work_>2097152) return false;
      }
      return false;
    };
    if (!search(search,boundary) || seed.size()+2!=boundary.size()) {
      strip_stop_="boundary-ear complete disk seed unavailable; states "+std::to_string(states);return false;
    }
    return true;
  }

  // Rebuild the COMPLETE authoritative mapped wire rather than an incomplete
  // old facet union. Original PCurves/native nodes and pole witnesses remain.
  bool restore_complete_export_face(const StripTrial& trial,int pole_choice,bool lookahead=false,bool longest=false,bool allow_flips=false,bool boundary_ears=false,
                                    bool retain_native_seed=false) {
    strip_stop_="whole-face preparation";
    const char* policy=boundary_ears ? "boundary-ears" : allow_flips ? "deferred-flip" : longest ? "longest" : lookahead ? "lookahead" : "original";
    try {
      if (trial.faces.size()!=1) { strip_stop_="whole-face requires one owner"; return false; }
      const auto& saved=trial.faces.front(); const auto face=saved.face;
      TopLoc_Location location; const auto mesh=BRep_Tool::Triangulation(face->GetFace(),location);
      if (face->WiresNb()!=1 || mesh.IsNull() || !mesh->HasUVNodes() || mesh->NbNodes()>65535 ||
          (face->GetStatusMask() & ~IMeshData_Outdated)!=0) { strip_stop_="whole-face unsupported chart/status"; return false; }
      // The merged-boundary owner already has a complete native trim mesh.
      // Its source angular failure must be refined, not accepted or replaced
      // by a sparse star over hundreds of retained boundary stations. The
      // proposal must first pass all existing geometry/domain/incidence gates;
      // only source-seven precision is deferred to the final refined leaves.
      if (retain_native_seed && (mesh->NbTriangles()>4096 || !validate_spherical_strip(trial,true,false))) {
        strip_stop_="native seed domain/cell scope: "+strip_stop_;return false;
      }
      std::map<int,int> aliases; IMeshData::IEdgePtr pole_edge=nullptr;
      if (!qualify_native_pole(saved,mesh,location,aliases,pole_edge,true,true,pole_choice)) return false;
      const auto canonical=[&](int id) { const auto found=aliases.find(id); return found==aliases.end() ? id : found->second; };
      const auto wire=face->GetWire(0); std::vector<int> boundary;std::map<int,double> boundary_tolerances;
      struct ChartSegment { IMeshData::IEdgePtr edge;IMeshData::IPCurveHandle pc;int first,last; };
      std::map<std::pair<int,int>,std::vector<ChartSegment>> chart_segments;
      if (wire->GetStatusMask()!=0) { strip_stop_="whole-face wire status"; return false; }
      for (int ei=0;ei<wire->EdgesNb();++ei) {
        const auto edge=wire->GetEdge(ei); const auto orientation=wire->GetEdgeOrientation(ei);
        const auto pc=edge->GetPCurve(face,orientation); const auto curve=edge->GetCurve();
        if ((orientation!=TopAbs_FORWARD && orientation!=TopAbs_REVERSED) || pc.IsNull() || pc->ParametersNb()<2 ||
            pc->ParametersNb()>(retain_native_seed ? 1024 : 256) || pc->ParametersNb()!=curve->ParametersNb() ||
            (BRep_Tool::Degenerated(edge->GetEdge()) && !certified_pole_edge(edge,pc,aliases)) ||
            (edge->GetDegenerated() && !BRep_Tool::Degenerated(edge->GetEdge()))) {
          strip_stop_="whole-face unsupported native constraint"; return false;
        }
        const int start=orientation==TopAbs_REVERSED ? pc->ParametersNb()-1 : 0;
        const int step=orientation==TopAbs_REVERSED ? -1 : 1;
        const auto next=wire->GetEdge((ei+1)%wire->EdgesNb())->GetPCurve(face,wire->GetEdgeOrientation((ei+1)%wire->EdgesNb()));
        const int end=start+step*(pc->ParametersNb()-1);
        if (next.IsNull() || next->ParametersNb()<2) { strip_stop_="whole-face missing next constraint"; return false; }
        const int next_start=wire->GetEdgeOrientation((ei+1)%wire->EdgesNb())==TopAbs_REVERSED ? next->ParametersNb()-1 : 0;
        if (canonical(pc->GetIndex(end))!=canonical(next->GetIndex(next_start)) ||
            pc->GetPoint(end).Distance(next->GetPoint(next_start))>Precision::PConfusion()) {
          strip_stop_="whole-face native wire junction"; return false;
        }
        for (int i=0;i<pc->ParametersNb();++i) {
          const int j=start+step*i,original=pc->GetIndex(j),id=canonical(original);
          if (++export_boundary_work_>2097152) { strip_stop_="whole-face work budget"; return false; }
          if (original<1 || original>mesh->NbNodes() || id<1 || id>mesh->NbNodes() ||
              !strip_finite(pc->GetPoint(j)) || !strip_finite(curve->GetPoint(j)) ||
              !strip_finite(mesh->UVNode(original)) || !strip_finite(mesh->Node(original)) ||
              mesh->UVNode(original).Distance(pc->GetPoint(j))>Precision::PConfusion() ||
              mesh->Node(original).Transformed(location.Transformation()).Distance(curve->GetPoint(j))>Precision::Confusion()) {
            strip_stop_="whole-face source node correspondence"; return false;
          }
          double tolerance=BRep_Tool::Tolerance(edge->GetEdge())+BRep_Tool::Tolerance(face->GetFace());
          if (j==0 || j+1==pc->ParametersNb()) {
            TopoDS_Vertex first,last;TopExp::Vertices(edge->GetEdge(),first,last);
            const auto vertex=j==0 ? first : last;
            if (!vertex.IsNull()) tolerance=std::max(tolerance,BRep_Tool::Tolerance(vertex)+BRep_Tool::Tolerance(face->GetFace()));
          }
          if (!std::isfinite(tolerance) || tolerance<0.0) { strip_stop_="whole-face invalid boundary tolerance";return false; }
          boundary_tolerances[id]=std::max(boundary_tolerances[id],tolerance);
          if (i) {
            const int previous=canonical(pc->GetIndex(j-step));
            if (previous!=id) chart_segments[{std::min(previous,id),std::max(previous,id)}].push_back({edge,pc,j-step,j});
          }
          if (i+1==pc->ParametersNb()) continue;
          if (boundary.empty() || boundary.back()!=id) boundary.push_back(id);
          if (boundary.size()>(retain_native_seed ? 1024 : 256)) { strip_stop_="whole-face boundary cap"; return false; }
        }
      }
      if (boundary.size()>1 && boundary.front()==boundary.back()) boundary.pop_back();
      if (boundary.size()<3 || std::set<int>(boundary.begin(),boundary.end()).size()!=boundary.size()) {
        strip_stop_="whole-face ambiguous quotient wire"; return false;
      }
      double area=0.0,xmin=std::numeric_limits<double>::infinity(),ymin=xmin,xmax=-xmin,ymax=-xmin;
      for (std::size_t i=0;i<boundary.size();++i) {
        const auto u=mesh->UVNode(boundary[i]),v=mesh->UVNode(boundary[(i+1)%boundary.size()]);
        if (!strip_finite(u) || !strip_finite(v) || u.Distance(v)==0.0) { strip_stop_="whole-face zero/nonfinite UV link"; return false; }
        area+=u.Coord().Crossed(v.Coord());
        xmin=std::min(xmin,u.X());xmax=std::max(xmax,u.X());ymin=std::min(ymin,u.Y());ymax=std::max(ymax,u.Y());
        for (std::size_t j=i+1;j<boundary.size();++j) {
          if (++export_boundary_work_>2097152) { strip_stop_="whole-face simplicity budget"; return false; }
          gp_Pnt2d hit;
          const auto flag=BRepMesh_GeomTool::IntSegSeg(u.Coord(),v.Coord(),mesh->UVNode(boundary[j]).Coord(),
              mesh->UVNode(boundary[(j+1)%boundary.size()]).Coord(),true,true,hit);
          const bool adjacent=j==i+1 || (i==0 && j+1==boundary.size());
          if (flag!=BRepMesh_GeomTool::NoIntersection && !(adjacent && flag==BRepMesh_GeomTool::EndPointTouch)) {
            std::ostringstream diagnostic,edge_details;diagnostic.precision(12);edge_details.precision(12);
            diagnostic << "chart intersection " << static_cast<int>(flag) << " segments " << i << '/' << j << " UV " << hit.X() << '/' << hit.Y();
            TopTools_IndexedMapOfShape native_edges;TopExp::MapShapes(GetModel()->GetShape(),TopAbs_EDGE,native_edges);
            std::vector<IMeshData::IEdgePtr> crossed_edges;std::vector<gp_Pnt> native_points;
            std::vector<Handle(Geom2d_Curve)> source_intervals;
            std::ostringstream continuous;continuous.precision(9);
            for (std::size_t segment : {i,j}) {
              const int a=boundary[segment],b=boundary[(segment+1)%boundary.size()];
              const auto found=chart_segments.find({std::min(a,b),std::max(a,b)});
              if (found==chart_segments.end() || found->second.size()!=1) { edge_details << " ambiguous native segment " << a << '/' << b;continue; }
              const auto& curve=found->second.front();crossed_edges.push_back(curve.edge);
              const auto first=mesh->UVNode(canonical(curve.pc->GetIndex(curve.first)));
              const auto last=mesh->UVNode(canonical(curve.pc->GetIndex(curve.last)));
              const auto delta=last.Coord()-first.Coord();const double length2=delta.SquareModulus();
              edge_details << "; edge " << native_edges.FindIndex(curve.edge->GetEdge())-1 << " samples " << curve.first << '/' << curve.last <<
                  " tol " << BRep_Tool::Tolerance(curve.edge->GetEdge()) << " owners " << curve.edge->PCurvesNb() <<
                  " same-param/range " << curve.edge->GetSameParam() << '/' << curve.edge->GetSameRange() <<
                  " interval " << curve.pc->GetParameter(curve.first) << '/' << curve.pc->GetParameter(curve.last) <<
                  " UVends " << first.X() << '/' << first.Y() << ':' << last.X() << '/' << last.Y();
              // Inspect the exact oriented PCurve over this sample interval;
              // chord-derived unequal samples alone do not establish whether
              // the continuous source boundaries intersect.
              try {
                const double t0=curve.pc->GetParameter(curve.first),t1=curve.pc->GetParameter(curve.last);
                double low,high;
                const auto source=BRep_Tool::CurveOnSurface(TopoDS::Edge(curve.edge->GetEdge().Oriented(curve.pc->GetOrientation())),
                    face->GetFace(),low,high);
                if (!source.IsNull() && std::isfinite(t0) && std::isfinite(t1) && std::isfinite(low) && std::isfinite(high) &&
                    t0!=t1 && std::min(t0,t1)>=low && std::max(t0,t1)<=high && export_curve_diagnostics_<16) {
                  source_intervals.push_back(new Geom2d_TrimmedCurve(source,std::min(t0,t1),std::max(t0,t1),true,false));
                  BRepAdaptor_Curve adaptor(TopoDS::Edge(curve.edge->GetEdge().Oriented(curve.pc->GetOrientation())),face->GetFace());
                  continuous << " source edge " << native_edges.FindIndex(curve.edge->GetEdge())-1 << " type/periodic " <<
                      static_cast<int>(adaptor.CurveOnSurface().GetCurve()->GetType()) << '/' << source->IsPeriodic() << " quarter chord-mm";
                  for (double fraction : {.25,.5,.75}) {
                    if (++export_boundary_work_>2097152) { continuous << " budget";break; }
                    const auto uv=source->Value(t0+(t1-t0)*fraction);
                    const gp_Pnt2d chord(first.Coord()*(1.0-fraction)+last.Coord()*fraction);
                    const auto exact=face->GetSurface()->Value(uv.X(),uv.Y()),sample=face->GetSurface()->Value(chord.X(),chord.Y());
                    if (!strip_finite(uv) || !strip_finite(exact) || !strip_finite(sample)) { continuous << " nonfinite";break; }
                    continuous << '/' << exact.Distance(sample);
                  }
                }
              } catch (const Standard_Failure&) { continuous << " source interval OCCT exception"; }
                catch (const std::exception&) { continuous << " source interval exception"; }
              if (flag!=BRepMesh_GeomTool::Cross || !std::isfinite(length2) || length2<=0.0 || !strip_finite(hit)) continue;
              const double fraction=delta.Dot(hit.Coord()-first.Coord())/length2;
              const double parameter=curve.pc->GetParameter(curve.first)+(curve.pc->GetParameter(curve.last)-curve.pc->GetParameter(curve.first))*fraction;
              if (!std::isfinite(fraction) || fraction<0.0 || fraction>1.0 || !std::isfinite(parameter)) continue;
              BRepAdaptor_Curve source_pc(TopoDS::Edge(curve.edge->GetEdge().Oriented(curve.pc->GetOrientation())),face->GetFace());
              const auto exact=source_pc.CurveOnSurface().GetCurve()->Value(parameter);
              if (!strip_finite(exact)) continue;
              const auto exact_point=face->GetSurface()->Value(exact.X(),exact.Y());
              const auto chord_point=face->GetSurface()->Value(hit.X(),hit.Y());
              if (!strip_finite(exact_point) || !strip_finite(chord_point)) continue;
              edge_details << " t " << parameter << " sourceUV " << exact.X() << '/' << exact.Y() <<
                  " chord-source-mm " << exact_point.Distance(chord_point);
              if (curve.edge->GetSameParam() && curve.edge->GetSameRange()) {
                const auto native=BRepAdaptor_Curve(curve.edge->GetEdge()).Value(parameter);
                if (!strip_finite(native)) continue;
                native_points.push_back(native);
                const auto sampled=curve.edge->GetCurve()->GetPoint(curve.first).XYZ()*(1.0-fraction)+
                    curve.edge->GetCurve()->GetPoint(curve.last).XYZ()*fraction;
                edge_details << " source/native/chord-gap " << exact_point.Distance(native) << '/' << gp_Pnt(sampled).Distance(native);
              }
            }
            if (source_intervals.size()==2 && export_curve_diagnostics_<16 && export_boundary_work_<=2097152) {
              ++export_curve_diagnostics_;
              try {
                Geom2dAPI_InterCurveCurve exact(source_intervals[0],source_intervals[1],Precision::PConfusion());
                const auto& result=exact.Intersector();
                diagnostic << "; continuous source intervals done/points/overlaps " << result.IsDone();
                if (result.IsDone()) diagnostic << '/' << result.NbPoints() << '/' << result.NbSegments();
              } catch (const Standard_Failure&) { diagnostic << "; continuous source interval OCCT exception"; }
                catch (const std::exception&) { diagnostic << "; continuous source interval exception"; }
            }
            diagnostic << continuous.str();
            if (native_points.size()==2) diagnostic << "; native-cross-distance " << native_points[0].Distance(native_points[1]);
            if (crossed_edges.size()==2 && flag==BRepMesh_GeomTool::Cross) {
              TopoDS_Vertex common;
              if (TopExp::CommonVertex(crossed_edges[0]->GetEdge(),crossed_edges[1]->GetEdge(),common))
                diagnostic << " common-vertex-cross-distance/tol " << face->GetSurface()->Value(hit.X(),hit.Y()).Distance(BRep_Tool::Pnt(common)) << '/' << BRep_Tool::Tolerance(common);
            }
            diagnostic << edge_details.str();
            std::fprintf(stderr,"Native export source chart face %d: %s\n",strip_original_faces_.FindIndex(face->GetFace())-1,
                diagnostic.str().substr(0,3000).c_str());
            strip_stop_=diagnostic.str().substr(0,1300);return false;
          }
        }
      }
      if (!std::isfinite(area) || area==0.0) { strip_stop_="whole-face zero/nonfinite domain area"; return false; }
      const double winding=area>0.0 ? 1.0 : -1.0;
      std::vector<gp_Pnt2d> kernel={{xmin,ymin},{xmax,ymin},{xmax,ymax},{xmin,ymax}};
      for (std::size_t i=0;i<boundary.size() && !retain_native_seed;++i) {
        const auto u=mesh->UVNode(boundary[i]),v=mesh->UVNode(boundary[(i+1)%boundary.size()]);
        const auto delta=v.Coord()-u.Coord();std::vector<gp_Pnt2d> clipped;
        for (std::size_t j=0;j<kernel.size();++j) {
          if (++export_boundary_work_>2097152) { strip_stop_="whole-face kernel budget";return false; }
          const auto x=kernel[j],y=kernel[(j+1)%kernel.size()];
          const double sx=winding*delta.Crossed(x.Coord()-u.Coord()),sy=winding*delta.Crossed(y.Coord()-u.Coord());
          if (!std::isfinite(sx) || !std::isfinite(sy)) { strip_stop_="whole-face nonfinite kernel"; return false; }
          if (sx>=0.0) clipped.push_back(x);
          if ((sx>=0.0)!=(sy>=0.0)) {
            const double fraction=sx/(sx-sy);
            if (!std::isfinite(fraction) || fraction<0.0 || fraction>1.0) { strip_stop_="whole-face kernel interpolation"; return false; }
            clipped.emplace_back(x.Coord()*(1.0-fraction)+y.Coord()*fraction);
          }
        }
        kernel=std::move(clipped);
        if (kernel.size()<3 || kernel.size()>64) { strip_stop_="whole-face no bounded star kernel"; return false; }
      }
      gp_XY center(0,0);for (const auto& u : kernel) center+=u.Coord();center/=static_cast<double>(kernel.size());
      std::vector<gp_Pnt2d> candidates={gp_Pnt2d(center)};
      for (std::size_t i=0;i<kernel.size() && candidates.size()<17;++i) {
        candidates.emplace_back(center*.75+kernel[i].Coord()*.25);
        if (candidates.size()<17) candidates.emplace_back(center*.5+kernel[i].Coord()*.5);
      }
      const double d=GetParameters().Deflection,angle=GetParameters().AngleInterior>0.0 ? GetParameters().AngleInterior : GetParameters().Angle;
      if (!std::isfinite(d) || d<=0.0 || !std::isfinite(angle) || angle<=0.0) { strip_stop_="whole-face invalid precision"; return false; }
      const double weights[7][3]={{1,0,0},{0,1,0},{0,0,1},{.5,.5,0},{0,.5,.5},{.5,0,.5},{1.0/3,1.0/3,1.0/3}};
      double best=std::numeric_limits<double>::infinity(),rejected=best,error_at_rejected=best;
      int worst_child=-1,worst_sample=-1;gp_Pnt2d selected,worst_uv;gp_Pnt selected_local;
      std::vector<std::array<int,3>> seed;
      if (retain_native_seed) {
        for (int ti=1;ti<=mesh->NbTriangles();++ti) {
          if (++export_boundary_work_>2097152) { strip_stop_="native seed read work cap";return false; }
          std::array<int,3> ids;mesh->Triangle(ti).Get(ids[0],ids[1],ids[2]);seed.push_back(ids);
        }
        best=0.0;
      } else if (boundary_ears) {
        if (!build_export_boundary_ears(face,mesh,location,boundary,winding,d,angle,seed)) return false;
        best=0.0;
      }
      for (const auto& candidate : candidates) {
        if (boundary_ears || retain_native_seed) break;
        if (!strip_finite(candidate) || BRepClass_FaceClassifier(face->GetFace(),candidate,Precision::PConfusion()).State()!=TopAbs_IN) continue;
        if (++export_boundary_work_>2097152) { strip_stop_="whole-face centre evaluation budget";return false; }
        const auto source_point=face->GetSurface()->Value(candidate.X(),candidate.Y());
        if (!strip_finite(source_point)) continue;
        const auto local_point=source_point.Transformed(location.Transformation().Inverted());
        const auto point=local_point.Transformed(location.Transformation());
        if (!strip_finite(local_point) || !strip_finite(point)) continue;
        bool valid=true;double max_angle=0.0,max_error=0.0;int ci=-1,si=-1;gp_Pnt2d failing_uv;
        for (std::size_t i=0;i<boundary.size() && valid;++i) {
          const gp_Pnt2d u[3]={mesh->UVNode(boundary[i]),mesh->UVNode(boundary[(i+1)%boundary.size()]),candidate};
          const gp_Pnt p[3]={mesh->Node(boundary[i]).Transformed(location.Transformation()),
              mesh->Node(boundary[(i+1)%boundary.size()]).Transformed(location.Transformation()),point};
          const auto normal=gp_Vec(p[0],p[1]).Crossed(gp_Vec(p[0],p[2]));
          const double child_area=winding*(u[1].Coord()-u[0].Coord()).Crossed(u[2].Coord()-u[0].Coord());
          if (!std::isfinite(child_area) || child_area<=0.0 || !std::isfinite(normal.SquareMagnitude()) || normal.SquareMagnitude()<=0.0) { valid=false;break; }
          int index=0;
          for (const auto& w : weights) {
            if (++export_boundary_work_>2097152) { strip_stop_="whole-face precision search budget";return false; }
            const gp_Pnt2d uv(u[0].Coord()*w[0]+u[1].Coord()*w[1]+u[2].Coord()*w[2]);
            const gp_Pnt affine(p[0].XYZ()*w[0]+p[1].XYZ()*w[1]+p[2].XYZ()*w[2]);gp_Pnt source;gp_Dir source_normal;
            if (!BRepMesh_GeomTool::Normal(face->GetSurface(),uv.X(),uv.Y(),source,source_normal) || !strip_finite(source)) { valid=false;break; }
            const double error=affine.Distance(source),angular=normal.Angle(gp_Vec(source_normal)*winding);
            if (!std::isfinite(error) || !std::isfinite(angular)) { valid=false;break; }
            max_error=std::max(max_error,error);
            if (angular>max_angle) { max_angle=angular;ci=static_cast<int>(i);si=index;failing_uv=uv; }++index;
          }
        }
        if (!valid) continue;
        if (max_angle<rejected) { rejected=max_angle;error_at_rejected=max_error;worst_child=ci;worst_sample=si;worst_uv=failing_uv; }
        const double score=std::max(max_angle/angle,max_error/d);
        if (std::isfinite(score) && score<best) { best=score;selected=candidate;selected_local=local_point; }
      }
      if (!std::isfinite(best)) {
        strip_stop_="whole-face no finite source star D/angle "+std::to_string(error_at_rejected)+"/"+std::to_string(rejected)+
            " child/sample "+std::to_string(worst_child)+"/"+std::to_string(worst_sample)+" UV "+
            std::to_string(worst_uv.X())+"/"+std::to_string(worst_uv.Y());return false;
      }
      const auto replacement=mesh->Copy();const int node=mesh->NbNodes()+1;
      if (!boundary_ears && !retain_native_seed) {
        replacement->ResizeNodes(node,true);replacement->SetUVNode(node,selected);
        replacement->SetNode(node,selected_local);
      }
      std::set<std::pair<int,int>> native_boundary_links;
      for (std::size_t i=0;i<boundary.size();++i) native_boundary_links.emplace(
          std::min(boundary[i],boundary[(i+1)%boundary.size()]),std::max(boundary[i],boundary[(i+1)%boundary.size()]));
      using Link=std::pair<int,int>;
      const auto link=[](int a,int b) { return std::make_pair(std::min(a,b),std::max(a,b)); };
      struct Cell { std::array<int,3> ids; int depth; bool active=true,qualified=false; };
      std::vector<Cell> cells;std::vector<int> pending;std::map<Link,std::set<int>> incidence;
      int active_count=0;
      const auto add_cell=[&](const std::array<int,3>& ids,int depth) {
        const int index=static_cast<int>(cells.size());cells.push_back({ids,depth});pending.push_back(index);++active_count;
        for (int i=0;i<3;++i) incidence[link(ids[i],ids[(i+1)%3])].insert(index);
      };
      const auto retire=[&](int index) {
        auto& cell=cells[index];cell.active=false;cell.qualified=false;--active_count;
        for (int i=0;i<3;++i) {
          const auto key=link(cell.ids[i],cell.ids[(i+1)%3]);auto found=incidence.find(key);
          if (found!=incidence.end()) { found->second.erase(index);if (found->second.empty()) incidence.erase(found); }
        }
      };
      if (boundary_ears || retain_native_seed) for (const auto& triangle : seed) add_cell(triangle,0);
      else for (std::size_t i=0;i<boundary.size();++i) add_cell({boundary[i],boundary[(i+1)%boundary.size()],node},0);
      int inspected=0,inserted=(boundary_ears || retain_native_seed) ? 0 : 1,max_depth=0,flips=0;
      std::set<std::pair<Link,Link>> flipped_diagonals;
      std::vector<std::string> recent_splits;
      // Bisect an unconstrained interior edge in BOTH incident cells. Retire
      // any prior neighbor certificate, and qualify all new cells again;
      // native boundary links remain unchanged and no hanging node survives.
      std::size_t pending_cursor=0;
      while (lookahead ? pending_cursor<pending.size() : !pending.empty()) {
        // The primary policy retains its qualified LIFO trajectory. Fair
        // processing in the independent alternate avoids exhausting one
        // radial lineage before its still-coarse neighboring owners.
        const int cell_index=lookahead ? pending[pending_cursor++] : pending.back();
        if (!lookahead) pending.pop_back();
        if (!cells[cell_index].active || cells[cell_index].qualified) continue;
        const auto cell=cells[cell_index];
        if (++inspected>8192 || active_count>4096 || cells.size()>16384) { strip_stop_="whole-face refinement cell budget";return false; }
        gp_Pnt2d u[3];gp_Pnt p[3];
        for (int i=0;i<3;++i) {
          u[i]=replacement->UVNode(cell.ids[i]);p[i]=replacement->Node(cell.ids[i]).Transformed(location.Transformation());
          if (!strip_finite(u[i]) || !strip_finite(p[i])) { strip_stop_="whole-face refinement nonfinite node";return false; }
        }
        const auto normal=gp_Vec(p[0],p[1]).Crossed(gp_Vec(p[0],p[2]));
        const double child_area=winding*(u[1].Coord()-u[0].Coord()).Crossed(u[2].Coord()-u[0].Coord());
        if (!std::isfinite(child_area) || child_area<=0.0 || !std::isfinite(normal.SquareMagnitude()) || normal.SquareMagnitude()<=0.0) {
          strip_stop_="whole-face refinement zero/native or inverted UV cell";return false;
        }
        double max_error=0.0,max_angle=0.0;int failing_sample=-1,index=0;gp_Vec endpoint_normals[3];
        double endpoint_errors[3]={},midpoint_errors[3]={};
        for (const auto& w : weights) {
          if (++export_boundary_work_>2097152) { strip_stop_="whole-face refinement source budget";return false; }
          const gp_Pnt2d uv(u[0].Coord()*w[0]+u[1].Coord()*w[1]+u[2].Coord()*w[2]);
          const gp_Pnt affine(p[0].XYZ()*w[0]+p[1].XYZ()*w[1]+p[2].XYZ()*w[2]);gp_Pnt source;gp_Dir source_normal;
          if (!BRepMesh_GeomTool::Normal(face->GetSurface(),uv.X(),uv.Y(),source,source_normal) || !strip_finite(source)) {
            strip_stop_="whole-face refinement undefined source normal";return false;
          }
          const double error=affine.Distance(source),angular=normal.Angle(gp_Vec(source_normal)*winding);
          if (!std::isfinite(error) || !std::isfinite(angular)) { strip_stop_="whole-face refinement nonfinite precision";return false; }
          if (index<3) {
            endpoint_normals[index]=gp_Vec(source_normal)*winding;
            endpoint_errors[index]=error;
            if (error>d) { strip_stop_="whole-face fixed vertex exceeds source distance "+std::to_string(error);return false; }
          }
          if (index>=3 && index<6) midpoint_errors[index-3]=error;
          max_error=std::max(max_error,error);max_angle=std::max(max_angle,angular);
          if (error>d || angular>angle) failing_sample=index;++index;
        }
        if (max_error<=d && max_angle<=angle) { cells[cell_index].qualified=true;continue; }
        const auto emit_precision_trace=[&](const std::string& reason) {
          std::ostringstream detail;detail.precision(12);
          detail << "Native export refinement face " << strip_original_faces_.FindIndex(face->GetFace())-1 <<
              " policy " << policy << " rejected " << reason;
          for (int j=0;j<3;++j) {
            const int next=(j+1)%3;
            const auto edge=gp_Vec(p[j],p[next]);const double length=edge.Magnitude();
            detail << "; node " << cell.ids[j] << " UV " << u[j].X() << '/' << u[j].Y() <<
                " XYZ " << p[j].X() << '/' << p[j].Y() << '/' << p[j].Z() << " source-normal " <<
                endpoint_normals[j].X() << '/' << endpoint_normals[j].Y() << '/' << endpoint_normals[j].Z() <<
                " source-gap " << endpoint_errors[j] << " next-edge XYZ/UV " << length << '/' << u[j].Distance(u[next]) <<
                " midpoint-D " << midpoint_errors[j] << " normal-separation " << endpoint_normals[j].Angle(endpoint_normals[next]) <<
                " native-constraint " << native_boundary_links.count(link(cell.ids[j],cell.ids[next]));
            if (std::isfinite(length) && length>0.0) detail << " minimum-angle " <<
                std::max(std::asin(std::min(1.0,std::abs((edge/length).Dot(endpoint_normals[j])))),
                    std::asin(std::min(1.0,std::abs((edge/length).Dot(endpoint_normals[next])))));
          }
          std::fprintf(stderr,"%s\n",detail.str().substr(0,5000).c_str());
        };
        int chosen=-1,interior_candidates=0;
        double best_edge=lookahead ? std::numeric_limits<double>::infinity() : -1.0,best_length=-1.0,best_uv_length=-1.0;
        std::vector<int> owners={cell_index};int depth=cell.depth;
        gp_Pnt2d selected_midpoint;gp_Pnt selected_midpoint_local;
        std::ostringstream alternatives;alternatives.precision(7);
        const int prospective_node=replacement->NbNodes()+1;
        const auto evaluate_child=[&](const std::array<int,3>& ids,const gp_Pnt2d& midpoint,const gp_Pnt& point,
                                      double& error,double& angular) {
          gp_Pnt2d uv[3];gp_Pnt xyz[3];
          for (int j=0;j<3;++j) {
            uv[j]=ids[j]==prospective_node ? midpoint : replacement->UVNode(ids[j]);
            xyz[j]=ids[j]==prospective_node ? point : replacement->Node(ids[j]).Transformed(location.Transformation());
            if (!strip_finite(uv[j]) || !strip_finite(xyz[j])) return false;
          }
          const auto normal=gp_Vec(xyz[0],xyz[1]).Crossed(gp_Vec(xyz[0],xyz[2]));
          const double area=winding*(uv[1].Coord()-uv[0].Coord()).Crossed(uv[2].Coord()-uv[0].Coord());
          if (!std::isfinite(area) || area<=0.0 || !std::isfinite(normal.SquareMagnitude()) || normal.SquareMagnitude()<=0.0) return false;
          for (const auto& w : weights) {
            if (++export_boundary_work_>2097152) return false;
            const gp_Pnt2d sample(uv[0].Coord()*w[0]+uv[1].Coord()*w[1]+uv[2].Coord()*w[2]);
            const gp_Pnt affine(xyz[0].XYZ()*w[0]+xyz[1].XYZ()*w[1]+xyz[2].XYZ()*w[2]);
            gp_Pnt source;gp_Dir source_normal;
            if (!BRepMesh_GeomTool::Normal(face->GetSurface(),sample.X(),sample.Y(),source,source_normal) ||
                !strip_finite(source) || !strip_finite(affine)) return false;
            const double gap=affine.Distance(source),turn=normal.Angle(gp_Vec(source_normal)*winding);
            if (!std::isfinite(gap) || !std::isfinite(turn)) return false;
            error=std::max(error,gap);angular=std::max(angular,turn);
          }
          return true;
        };
        bool flipped=false;
        if (longest && allow_flips && flips<32 && cells.size()+2<=16384) for (int i=0;i<3 && !flipped;++i) {
          const int a=cell.ids[i],b=cell.ids[(i+1)%3],c=cell.ids[(i+2)%3];const auto old_link=link(a,b);
          if (native_boundary_links.count(old_link)) continue;
          const auto found=incidence.find(old_link);
          if (found==incidence.end() || found->second.size()!=2 || !found->second.count(cell_index)) continue;
          int other=-1;
          for (int owner : found->second) if (owner!=cell_index) other=owner;
          const auto neighbor=cells[other];int fourth=-1;
          for (int j=0;j<3;++j) if (neighbor.ids[j]==b && neighbor.ids[(j+1)%3]==a) fourth=neighbor.ids[(j+2)%3];
          if (fourth<0 || std::set<int>{a,b,c,fourth}.size()!=4) continue;
          const auto new_link=link(c,fourth);
          const auto pair=old_link<new_link ? std::make_pair(old_link,new_link) : std::make_pair(new_link,old_link);
          if (native_boundary_links.count(new_link) || incidence.count(new_link) || flipped_diagonals.count(pair)) continue;
          const int quad[4]={b,c,a,fourth};bool convex=true;
          for (int j=0;j<4;++j) {
            if (++export_boundary_work_>2097152) { strip_stop_="whole-face edge flip work cap";return false; }
            convex &= winding*certified_strip_orientation(replacement->UVNode(quad[j]),
                replacement->UVNode(quad[(j+1)%4]),replacement->UVNode(quad[(j+2)%4]))>0.0;
          }
          if (!convex) continue;
          const std::array<int,3> first={c,a,fourth},second={fourth,b,c};
          std::map<Link,int> old_boundary,new_boundary;
          const auto accumulate=[&](std::map<Link,int>& boundary,const std::array<int,3>& ids) {
            for (int j=0;j<3;++j) boundary[link(ids[j],ids[(j+1)%3])]+=ids[j]<ids[(j+1)%3] ? 1 : -1;
            for (auto it=boundary.begin();it!=boundary.end();) if (it->second==0) it=boundary.erase(it);else ++it;
          };
          accumulate(old_boundary,cell.ids);accumulate(old_boundary,neighbor.ids);
          accumulate(new_boundary,first);accumulate(new_boundary,second);
          if (old_boundary!=new_boundary) continue;
          const auto signed_area=[&](const std::array<int,3>& ids) {
            const auto p=replacement->UVNode(ids[0]),q=replacement->UVNode(ids[1]),r=replacement->UVNode(ids[2]);
            return (q.Coord()-p.Coord()).Crossed(r.Coord()-p.Coord());
          };
          double coordinate_scale=0.0;
          for (int id : quad) { const auto uv=replacement->UVNode(id);coordinate_scale+=std::abs(uv.X())+std::abs(uv.Y()); }
          const double area_delta=std::abs(signed_area(cell.ids)+signed_area(neighbor.ids)-signed_area(first)-signed_area(second));
          if (!std::isfinite(area_delta) || area_delta>128.0*std::numeric_limits<double>::epsilon()*coordinate_scale*coordinate_scale) continue;
          double error=0.0,angular=0.0;
          const bool valid_first=evaluate_child(first,gp_Pnt2d(),gp_Pnt(),error,angular);
          const bool valid_second=evaluate_child(second,gp_Pnt2d(),gp_Pnt(),error,angular);
          if (export_boundary_work_>2097152) { strip_stop_="whole-face edge flip source cap";return false; }
          // Both replacement cells qualify outright. A currently failing cell
          // is eliminated, so no geometric acceptance comes from a quality
          // heuristic or resetting its refinement lineage.
          if (!valid_first || !valid_second || error>d || angular>angle) continue;
          const int lineage=std::max(cell.depth,neighbor.depth);
          retire(cell_index);retire(other);
          add_cell(first,lineage);cells.back().qualified=true;
          add_cell(second,lineage);cells.back().qualified=true;
          flipped_diagonals.insert(pair);++flips;flipped=true;
        }
        if (flipped) continue;
        for (int i=0;i<3;++i) {
          const int next=(i+1)%3,a=cell.ids[i],b=cell.ids[next];
          const auto key=link(a,b);
          if (native_boundary_links.count(key)) {
            const gp_Vec edge(p[i],p[next]);const double length=edge.Magnitude();
            if (!std::isfinite(length) || length<=0.0) { strip_stop_="whole-face native boundary has zero/nonfinite length";return false; }
            const gp_Vec unit=edge/length;
            const double minimum=std::max(std::asin(std::min(1.0,std::abs(unit.Dot(endpoint_normals[i])))),
                std::asin(std::min(1.0,std::abs(unit.Dot(endpoint_normals[next])))));
            if (minimum>angle || endpoint_normals[i].Angle(endpoint_normals[next])>2.0*angle) {
              strip_stop_="native edge "+std::to_string(a)+"/"+std::to_string(b)+" minimum angle "+std::to_string(minimum)+
                  " endpoint source-gap/tol "+std::to_string(endpoint_errors[i])+"/"+std::to_string(boundary_tolerances[a])+
                  " "+std::to_string(endpoint_errors[next])+"/"+std::to_string(boundary_tolerances[b])+" UV "+
                  std::to_string(u[i].X())+"/"+std::to_string(u[i].Y())+" -> "+std::to_string(u[next].X())+"/"+std::to_string(u[next].Y());return false;
            }
            continue;
          }
          ++interior_candidates;
          const auto found=incidence.find(key);
          if (found==incidence.end() || found->second.size()!=2) { strip_stop_="whole-face interior split lacks exactly two owners";return false; }
          const std::vector<int> trial_owners(found->second.begin(),found->second.end());int balance=0,trial_depth=0;
          for (int owner : trial_owners) {
            trial_depth=std::max(trial_depth,cells[owner].depth);
            for (int i=0;i<3;++i) if (link(cells[owner].ids[i],cells[owner].ids[(i+1)%3])==key)
              balance+=cells[owner].ids[i]<cells[owner].ids[(i+1)%3] ? 1 : -1;
          }
          if (balance!=0) { strip_stop_="whole-face split owners are not oppositely oriented";return false; }
          if (!lookahead) {
            const double score=std::max(midpoint_errors[i]/d,endpoint_normals[i].Angle(endpoint_normals[next])/angle);
            const double length=p[i].Distance(p[next]);
            if (!std::isfinite(score) || !std::isfinite(length)) { strip_stop_="whole-face nonfinite interior edge metric";return false; }
            alternatives << " [edge " << a << '/' << b << " owner-depths " << cells[trial_owners[0]].depth << '/' <<
                cells[trial_owners[1]].depth << " score " << score << " XYZ/UV-length " << length << '/' << u[i].Distance(u[next]) <<
                " midpoint-D " << midpoint_errors[i] << " normal-separation " << endpoint_normals[i].Angle(endpoint_normals[next]) << ']';
            if (score>best_edge || (score==best_edge && length>best_length)) {
              chosen=i;best_edge=score;best_length=length;owners=trial_owners;depth=trial_depth;
            }
            continue;
          }
          alternatives << " [edge " << a << '/' << b << " owner-depths " << cells[trial_owners[0]].depth << '/' <<
              cells[trial_owners[1]].depth << " xyz/UV-length " << p[i].Distance(p[next]) << '/' << u[i].Distance(u[next]) <<
              " midpoint-error " << midpoint_errors[i];
          if (trial_depth>=8) { alternatives << " lineage cap]";continue; }
          const gp_Pnt2d midpoint((u[i].Coord()+u[next].Coord())*.5);
          if (!strip_finite(midpoint) || BRepClass_FaceClassifier(face->GetFace(),midpoint,Precision::PConfusion()).State()!=TopAbs_IN) {
            alternatives << " outside original trim]";continue;
          }
          if (++export_boundary_work_>2097152) { strip_stop_="whole-face split lookahead source budget";return false; }
          const auto source=face->GetSurface()->Value(midpoint.X(),midpoint.Y());
          const auto local=source.Transformed(location.Transformation().Inverted()),point=local.Transformed(location.Transformation());
          if (!strip_finite(source) || !strip_finite(local) || !strip_finite(point)) { alternatives << " nonfinite midpoint]";continue; }
          double trial_error=0.0,trial_angle=0.0;bool valid=true;
          for (int owner : trial_owners) {
            const auto& ids=cells[owner].ids;bool matched=false;
            for (int j=0;j<3;++j) if (link(ids[j],ids[(j+1)%3])==key) {
              matched=true;
              valid &= evaluate_child({ids[j],prospective_node,ids[(j+2)%3]},midpoint,point,trial_error,trial_angle);
              valid &= evaluate_child({prospective_node,ids[(j+1)%3],ids[(j+2)%3]},midpoint,point,trial_error,trial_angle);
              break;
            }
            if (!matched) { strip_stop_="whole-face lookahead lost an owner";return false; }
          }
          if (export_boundary_work_>2097152) { strip_stop_="whole-face split child witness budget";return false; }
          const double score=std::max(trial_error/d,trial_angle/angle),length=p[i].Distance(p[next]);
          const double uv_length=u[i].Distance(u[next]);
          alternatives << " children D/angle/score " << trial_error << '/' << trial_angle << '/' << score << (valid ? "]" : " invalid]");
          const bool better=longest ? length>best_length || (length==best_length && uv_length>best_uv_length) :
              score<best_edge || (score==best_edge && length>best_length);
          if (valid && std::isfinite(score) && std::isfinite(length) && std::isfinite(uv_length) && better) {
            chosen=i;best_edge=score;best_length=length;owners=trial_owners;depth=trial_depth;
            best_uv_length=uv_length;
            selected_midpoint=midpoint;selected_midpoint_local=local;
          }
        }
        if (chosen<0 && interior_candidates) {
          strip_stop_="cell "+std::to_string(cell.ids[0])+"/"+std::to_string(cell.ids[1])+"/"+std::to_string(cell.ids[2])+
              " no certified two-owner split policy "+policy+"; D/angle "+
              std::to_string(max_error)+"/"+std::to_string(max_angle)+alternatives.str();
          for (const auto& split : recent_splits) strip_stop_+="; "+split;
          emit_precision_trace(strip_stop_);
          return false;
        }
        if (depth>=8 || inserted>=4096 || replacement->NbNodes()>=65536 || active_count+2>4096 || cells.size()+4>16384) {
          strip_stop_="cell "+std::to_string(cell.ids[0])+"/"+std::to_string(cell.ids[1])+"/"+std::to_string(cell.ids[2])+
              " policy "+policy+" depth/nodes "+std::to_string(depth)+"/"+
              std::to_string(inserted)+" D/angle "+
              std::to_string(max_error)+"/"+std::to_string(max_angle)+" sample "+std::to_string(failing_sample)+
              " split "+(chosen>=0 ? std::to_string(cell.ids[chosen])+"/"+std::to_string(cell.ids[(chosen+1)%3]) : "centroid")+
              " endpoint source-gaps "+std::to_string(endpoint_errors[0])+"/"+std::to_string(endpoint_errors[1])+"/"+std::to_string(endpoint_errors[2])+
              " owner-depths "+std::to_string(cells[owners.front()].depth)+"/"+std::to_string(cells[owners.back()].depth)+
              " split score "+std::to_string(best_edge)+alternatives.str();
          for (const auto& split : recent_splits) strip_stop_+="; "+split;
          emit_precision_trace(strip_stop_);
          return false;
        }
        const gp_Pnt2d uv(chosen>=0 ? (lookahead ? selected_midpoint : gp_Pnt2d((u[chosen].Coord()+u[(chosen+1)%3].Coord())*.5)) :
            gp_Pnt2d((u[0].Coord()+u[1].Coord()+u[2].Coord())/3.0));
        if (!strip_finite(uv) || BRepClass_FaceClassifier(face->GetFace(),uv,Precision::PConfusion()).State()!=TopAbs_IN) {
          strip_stop_="whole-face refinement centroid outside source trim";return false;
        }
        if (++export_boundary_work_>2097152) { strip_stop_="whole-face refinement centre budget";return false; }
        const auto local=chosen>=0 && lookahead ? selected_midpoint_local :
            face->GetSurface()->Value(uv.X(),uv.Y()).Transformed(location.Transformation().Inverted());
        if (!strip_finite(local) || !strip_finite(local.Transformed(location.Transformation()))) { strip_stop_="whole-face refinement nonfinite source point";return false; }
        const int next=replacement->NbNodes()+1;replacement->ResizeNodes(next,true);
        replacement->SetUVNode(next,uv);replacement->SetNode(next,local);++inserted;max_depth=std::max(max_depth,depth+1);
        recent_splits.push_back("split "+(chosen>=0 ? std::to_string(cell.ids[chosen])+"/"+std::to_string(cell.ids[(chosen+1)%3]) : "centroid")+
            " owner-depths "+std::to_string(cells[owners.front()].depth)+"/"+std::to_string(cells[owners.back()].depth)+
            " score "+std::to_string(best_edge));
        if (recent_splits.size()>8) recent_splits.erase(recent_splits.begin());
        for (int owner : owners) {
          const auto old=cells[owner];retire(owner);
          const int child_depth=(lookahead ? old.depth : depth)+1;
          if (chosen<0) { for (int i=0;i<3;++i) add_cell({old.ids[i],old.ids[(i+1)%3],next},child_depth);continue; }
          const auto key=link(cell.ids[chosen],cell.ids[(chosen+1)%3]);bool found=false;
          for (int i=0;i<3;++i) if (link(old.ids[i],old.ids[(i+1)%3])==key) {
            add_cell({old.ids[i],next,old.ids[(i+2)%3]},child_depth);
            add_cell({next,old.ids[(i+1)%3],old.ids[(i+2)%3]},child_depth);found=true;break;
          }
          if (!found) { strip_stop_="whole-face split lost its native owner edge";return false; }
        }
      }
      std::vector<std::array<int,3>> complete;
      for (const auto& cell : cells) if (cell.active) {
        if (!cell.qualified) { strip_stop_="whole-face active cell lacks source certificate";return false; }
        complete.push_back(cell.ids);
      }
      if (complete.empty() || complete.size()>4096) { strip_stop_="whole-face refinement final facet budget";return false; }
      replacement->ResizeTriangles(static_cast<int>(complete.size()),false);
      for (std::size_t i=0;i<complete.size();++i) replacement->SetTriangle(static_cast<int>(i)+1,
          Poly_Triangle(complete[i][0],complete[i][1],complete[i][2]));
      replacement->RemoveNormals();replacement->ComputeNormals();BRep_Builder().UpdateFace(face->GetFace(),replacement);
      strip_stop_="whole-face refined star installed facets/nodes/depth "+std::to_string(complete.size())+"/"+
          std::to_string(inserted)+"/"+std::to_string(max_depth)+" flips "+std::to_string(flips)+" for full source certificate";return true;
    } catch (const Standard_Failure&) { strip_stop_="OCCT exception proposing whole-face star";return false; }
      catch (const std::exception&) { strip_stop_="exception proposing whole-face star";return false; }
  }

  // Two curved face owners can triangulate the same boundary-node ear. Their
  // otherwise legitimate internal diagonal then receives four global uses.
  // Refine that interior link with a face-owned exact surface sample, retaining
  // every original positive UV region and all shared native boundary samples.
  void recover_export_internal_diagonals() {
    struct RollbackFailure : std::runtime_error { using std::runtime_error::runtime_error; };
    int attempts=0,repaired=0;
    try {
    using Link=std::pair<int,int>;
    struct Incidence { int count=0,balance=0; std::array<int,2> triangles; };
    struct FaceLinks { Handle(Poly_Triangulation) mesh; TopLoc_Location location; std::map<Link,Incidence> links; };
    std::map<IMeshData::IFacePtr,FaceLinks> cache;
    std::size_t work=0;
    const auto link=[](int a,int b) { return std::make_pair(std::min(a,b),std::max(a,b)); };
    const auto obtain=[&](IMeshData::IFacePtr face)->FaceLinks* {
      const auto found=cache.find(face); if (found!=cache.end()) return &found->second;
      if ((face->GetStatusMask() & ~IMeshData_Outdated)!=0) return nullptr;
      FaceLinks value; value.mesh=BRep_Tool::Triangulation(face->GetFace(),value.location);
      if (value.mesh.IsNull() || !value.mesh->HasUVNodes() || value.mesh->NbNodes()>4096 ||
          value.mesh->NbTriangles()<1 || value.mesh->NbTriangles()>4096 ||
          work+3*static_cast<std::size_t>(value.mesh->NbTriangles())>2097152) return nullptr;
      for (int ti=1;ti<=value.mesh->NbTriangles();++ti) {
        int ids[3]; value.mesh->Triangle(ti).Get(ids[0],ids[1],ids[2]);
        for (int id : ids) if (id<1 || id>value.mesh->NbNodes()) return nullptr;
        for (int i=0;i<3;++i) {
          ++work; auto& use=value.links[link(ids[i],ids[(i+1)%3])];
          if (use.count<2) use.triangles[use.count]=ti;
          ++use.count; use.balance+=ids[i]<ids[(i+1)%3] ? 1 : -1;
        }
      }
      return &cache.emplace(face,std::move(value)).first->second;
    };
    struct Candidate { IMeshData::IEdgePtr edge; IMeshData::IPCurveHandle first,second; int from,to; };
    std::vector<Candidate> candidates;
    const auto& model=GetModel();
    for (int ei=0;ei<model->EdgesNb() && candidates.size()<16 && work<2097152;++ei) {
      const auto edge=model->GetEdge(ei);
      if (edge->PCurvesNb()!=2 || BRep_Tool::Degenerated(edge->GetEdge()) || edge->GetDegenerated() ||
          !edge->GetSameParam() || !edge->GetSameRange()) continue;
      auto first=edge->GetPCurve(0),second=edge->GetPCurve(1);
      if (first->GetFace()==second->GetFace() || first->ParametersNb()<3 || first->ParametersNb()>64 ||
          first->ParametersNb()!=second->ParametersNb() || first->ParametersNb()!=edge->GetCurve()->ParametersNb()) continue;
      const int fi=strip_original_faces_.FindIndex(first->GetFace()->GetFace());
      const int si=strip_original_faces_.FindIndex(second->GetFace()->GetFace());
      if (fi<1 || si<1) continue;
      if (si<fi) std::swap(first,second);
      const auto oriented=[&](const IMeshData::IPCurveHandle& pc) {
        const auto orientation=pc->GetOrientation();
        const auto face_orientation=strip_original_faces_.FindKey(strip_original_faces_.FindIndex(pc->GetFace()->GetFace())).Orientation();
        if ((orientation!=TopAbs_FORWARD && orientation!=TopAbs_REVERSED) ||
            (face_orientation!=TopAbs_FORWARD && face_orientation!=TopAbs_REVERSED)) return 0;
        return (orientation==TopAbs_FORWARD ? 1 : -1)*(face_orientation==TopAbs_FORWARD ? 1 : -1);
      };
      if (!oriented(first) || oriented(first)+oriented(second)!=0) continue;
      auto* a=obtain(first->GetFace()); auto* b=obtain(second->GetFace()); if (!a || !b) continue;
      std::map<int,int> slots;
      std::set<int> ambiguous;
      for (int i=0;i<first->ParametersNb();++i) if (!slots.emplace(first->GetIndex(i),i).second) ambiguous.insert(first->GetIndex(i));
      for (const auto& entry : a->links) {
        if (entry.second.count!=2 || entry.second.balance!=0 || !slots.count(entry.first.first) || !slots.count(entry.first.second) ||
            ambiguous.count(entry.first.first) || ambiguous.count(entry.first.second)) continue;
        const int from=std::min(slots.at(entry.first.first),slots.at(entry.first.second));
        const int to=std::max(slots.at(entry.first.first),slots.at(entry.first.second));
        if (to-from<2) continue;
        const auto other=b->links.find(link(second->GetIndex(from),second->GetIndex(to)));
        if (other==b->links.end() || other->second.count!=2 || other->second.balance!=0) continue;
        candidates.push_back({edge.get(),first,second,from,to});
        if (candidates.size()==16) break;
      }
    }
    TopTools_IndexedMapOfShape native_edges; TopExp::MapShapes(model->GetShape(),TopAbs_EDGE,native_edges);
    for (const auto& candidate : candidates) {
      auto* face=candidate.first->GetFace();
      const int original=strip_original_faces_.FindIndex(face->GetFace());
      if (original<1) throw std::runtime_error("internal diagonal face lacks original topology mapping");
      auto& why=export_boundary_rejections_[original-1];
      const auto* source=obtain(face);
      const auto* other=obtain(candidate.second->GetFace());
      if (!source || !other) { why="internal diagonal incidence/work budget"; continue; }
      const int a=candidate.first->GetIndex(candidate.from),b=candidate.first->GetIndex(candidate.to);
      const auto use=source->links.find(link(a,b));
      const auto opposite=other->links.find(link(candidate.second->GetIndex(candidate.from),candidate.second->GetIndex(candidate.to)));
      if (use==source->links.end() || opposite==other->links.end() || use->second.count!=2 || opposite->second.count!=2 ||
          use->second.balance || opposite->second.balance) continue;
      bool located_alias=false;
      for (int fi=1;fi<=strip_original_faces_.Extent();++fi) {
        const auto& f=strip_original_faces_.FindKey(fi);
        if (f.IsPartner(face->GetFace()) && !f.IsSame(face->GetFace())) { located_alias=true; break; }
      }
      if (located_alias) { why="internal diagonal has located face aliases"; continue; }
      const auto mesh=source->mesh; const auto location=source->location;
      StripFace saved{face,face->GetStatusMask(),{},mesh,{}};
      saved.original_orientation=strip_original_faces_.FindKey(original).Orientation();
      StripTrial trial; trial.faces.push_back(saved);
      ++attempts;
      bool accepted=false;
      std::string stage="preparing complete source UV patch";
      try {
        std::set<Link> constraints;
        for (int wi=0;wi<face->WiresNb();++wi) {
          const auto wire=face->GetWire(wi);
          for (int ei=0;ei<wire->EdgesNb();++ei) {
            const auto pc=wire->GetEdge(ei)->GetPCurve(face,wire->GetEdgeOrientation(ei));
            if (pc.IsNull() || pc->ParametersNb()>4096) throw std::runtime_error("patch lacks bounded native constraints");
            for (int i=1;i<pc->ParametersNb();++i) constraints.insert(link(pc->GetIndex(i-1),pc->GetIndex(i)));
          }
        }
        const auto inverted=[&](int ti) {
          if (++work>2097152) throw std::runtime_error("patch source-normal inspection budget");
          int ids[3]; mesh->Triangle(ti).Get(ids[0],ids[1],ids[2]);
          const auto u=mesh->UVNode(ids[0]),v=mesh->UVNode(ids[1]),w=mesh->UVNode(ids[2]);
          const gp_Pnt2d center((u.Coord()+v.Coord()+w.Coord())/3.0);
          gp_Pnt p; gp_Vec du,dv; face->GetSurface()->D1(center.X(),center.Y(),p,du,dv);
          const auto source_normal=du.Crossed(dv);
          const auto x=mesh->Node(ids[0]).Transformed(location.Transformation());
          const auto y=mesh->Node(ids[1]).Transformed(location.Transformation());
          const auto z=mesh->Node(ids[2]).Transformed(location.Transformation());
          const double dot=gp_Vec(x,y).Crossed(gp_Vec(x,z)).Dot(source_normal);
          return std::isfinite(source_normal.SquareMagnitude()) && source_normal.SquareMagnitude()>0.0 &&
              std::isfinite(dot) && dot<0.0;
        };
        std::set<int> patch(use->second.triangles.begin(),use->second.triangles.end());
        // An inverted ear cannot be fixed by subdividing its fixed boundary.
        // Expand only through unconstrained interior links to connected bad
        // cells and one adjacent positive cell supporting a larger boundary.
        bool expanded=true;
        while (expanded && patch.size()<8) {
          expanded=false;
          const auto current=patch;
          for (int ti : current) {
            int ids[3]; mesh->Triangle(ti).Get(ids[0],ids[1],ids[2]);
            for (int i=0;i<3 && patch.size()<8;++i) {
              const auto key=link(ids[i],ids[(i+1)%3]); const auto neighbour=source->links.find(key);
              if (constraints.count(key) || neighbour==source->links.end() || neighbour->second.count!=2 || neighbour->second.balance) continue;
              for (int other_ti : neighbour->second.triangles) if (!patch.count(other_ti) && inverted(other_ti)) {
                patch.insert(other_ti); expanded=true;
              }
            }
          }
        }
        bool has_inversion=false;
        for (int ti : patch) has_inversion=has_inversion || inverted(ti);
        if (patch.size()<8 && has_inversion) {
          int best=0; double best_area=0.0;
          for (int ti : patch) {
            int ids[3]; mesh->Triangle(ti).Get(ids[0],ids[1],ids[2]);
            for (int i=0;i<3;++i) {
              const auto key=link(ids[i],ids[(i+1)%3]); const auto neighbour=source->links.find(key);
              if (constraints.count(key) || neighbour==source->links.end() || neighbour->second.count!=2 || neighbour->second.balance) continue;
              for (int other_ti : neighbour->second.triangles) if (!patch.count(other_ti) && !inverted(other_ti)) {
                int n[3]; mesh->Triangle(other_ti).Get(n[0],n[1],n[2]);
                const auto u=mesh->UVNode(n[0]),v=mesh->UVNode(n[1]),w=mesh->UVNode(n[2]);
                const double area=.5*(v.Coord()-u.Coord()).Crossed(w.Coord()-u.Coord());
                if (std::isfinite(area) && area>best_area) { best_area=area; best=other_ti; }
              }
            }
          }
          if (best) patch.insert(best);
        }
        std::vector<std::array<int,3>> children;
        std::map<Link,int> old_links,new_links,old_directions,new_directions;
        double old_area=0.0,new_area=0.0,area_scale=0.0;
        for (int ti : patch) {
          int ids[3]; mesh->Triangle(ti).Get(ids[0],ids[1],ids[2]);
          for (int i=0;i<3;++i) {
            const auto key=link(ids[i],ids[(i+1)%3]); ++old_links[key]; old_directions[key]+=ids[i]<ids[(i+1)%3] ? 1 : -1;
          }
          const auto x=mesh->UVNode(ids[0]),y=mesh->UVNode(ids[1]),z=mesh->UVNode(ids[2]);
          const double area=.5*(y.Coord()-x.Coord()).Crossed(z.Coord()-x.Coord());
          if (!std::isfinite(area) || area<=0.0) throw std::runtime_error("old internal patch has nonpositive UV area");
          const auto px=mesh->Node(ids[0]).Transformed(location.Transformation());
          const auto py=mesh->Node(ids[1]).Transformed(location.Transformation());
          const auto pz=mesh->Node(ids[2]).Transformed(location.Transformation());
          const double native_area=gp_Vec(px,py).Crossed(gp_Vec(px,pz)).SquareMagnitude();
          if (!strip_finite(px) || !strip_finite(py) || !strip_finite(pz) || !std::isfinite(native_area) || native_area<=0.0)
            throw std::runtime_error("old internal patch has zero/nonfinite native area");
          old_area+=area;
        }
        std::map<int,int> next; std::set<int> incoming;
        for (const auto& entry : old_links) {
          if (entry.second==2 && old_directions[entry.first]==0) continue;
          if (entry.second!=1 || std::abs(old_directions[entry.first])!=1) throw std::runtime_error("old patch is not an oriented disk");
          const int from=old_directions[entry.first]>0 ? entry.first.first : entry.first.second;
          const int to=old_directions[entry.first]>0 ? entry.first.second : entry.first.first;
          if (!next.emplace(from,to).second || !incoming.insert(to).second) throw std::runtime_error("patch outer boundary branches");
        }
        if (next.size()<3 || next.size()>24) throw std::runtime_error("patch boundary exceeds bounded disk scope");
        std::vector<int> boundary; int cursor=next.begin()->first;
        do {
          if (!next.count(cursor) || boundary.size()>=next.size()) throw std::runtime_error("patch has multiple boundary cycles");
          boundary.push_back(cursor); cursor=next.at(cursor);
        } while (cursor!=boundary.front());
        if (boundary.size()!=next.size()) throw std::runtime_error("patch has a hole");
        double xmin=std::numeric_limits<double>::infinity(),ymin=xmin,xmax=-xmin,ymax=-xmin;
        for (std::size_t i=0;i<boundary.size();++i) {
          const auto u=mesh->UVNode(boundary[i]),v=mesh->UVNode(boundary[(i+1)%boundary.size()]);
          if (!strip_finite(u) || !strip_finite(v) || u.Distance(v)==0.0) throw std::runtime_error("patch UV boundary is degenerate");
          xmin=std::min(xmin,u.X()); xmax=std::max(xmax,u.X()); ymin=std::min(ymin,u.Y()); ymax=std::max(ymax,u.Y());
          for (std::size_t j=i+1;j<boundary.size();++j) {
            gp_Pnt2d hit;
            const auto flag=BRepMesh_GeomTool::IntSegSeg(u.Coord(),v.Coord(),mesh->UVNode(boundary[j]).Coord(),
                mesh->UVNode(boundary[(j+1)%boundary.size()]).Coord(),true,true,hit);
            const bool adjacent=j==i+1 || (i==0 && j+1==boundary.size());
            if (flag!=BRepMesh_GeomTool::NoIntersection && !(adjacent && flag==BRepMesh_GeomTool::EndPointTouch))
              throw std::runtime_error("expanded source UV patch is not simple");
          }
        }
        // The intersection of oriented boundary half-planes is precisely the
        // star kernel. A strictly positive fan at its center covers the same
        // simple disk once, including the entire original inverted ear.
        std::vector<gp_Pnt2d> kernel={{xmin,ymin},{xmax,ymin},{xmax,ymax},{xmin,ymax}};
        for (std::size_t i=0;i<boundary.size();++i) {
          const auto u=mesh->UVNode(boundary[i]),v=mesh->UVNode(boundary[(i+1)%boundary.size()]);
          const auto delta=v.Coord()-u.Coord(); std::vector<gp_Pnt2d> clipped;
          for (std::size_t j=0;j<kernel.size();++j) {
            const auto x=kernel[j],y=kernel[(j+1)%kernel.size()];
            const double sx=delta.Crossed(x.Coord()-u.Coord()),sy=delta.Crossed(y.Coord()-u.Coord());
            if (!std::isfinite(sx) || !std::isfinite(sy)) throw std::runtime_error("patch kernel is nonfinite");
            if (sx>=0.0) clipped.push_back(x);
            if ((sx>=0.0)!=(sy>=0.0)) {
              const double fraction=sx/(sx-sy);
              if (!std::isfinite(fraction) || fraction<0.0 || fraction>1.0) throw std::runtime_error("invalid patch kernel intersection");
              clipped.emplace_back(x.Coord()*(1.0-fraction)+y.Coord()*fraction);
            }
          }
          kernel=std::move(clipped);
          if (kernel.size()<3 || kernel.size()>64) throw std::runtime_error("expanded patch has no bounded star kernel");
        }
        gp_XY center(0,0); for (const auto& u : kernel) center+=u.Coord(); center/=static_cast<double>(kernel.size());
        const double d=GetParameters().Deflection;
        const double angle=GetParameters().AngleInterior>0.0 ? GetParameters().AngleInterior : GetParameters().Angle;
        if (!std::isfinite(d) || d<=0.0 || !std::isfinite(angle) || angle<=0.0) throw std::runtime_error("invalid native precision");
        // The kernel mean need not minimize source-normal error on a distorted
        // imported chart. Convex combinations with kernel vertices stay in the
        // same certified star domain; try a bounded set without moving any
        // original node or accepting a child beyond the requested precision.
        std::vector<gp_Pnt2d> star_samples={gp_Pnt2d(center)};
        for (std::size_t i=0;i<kernel.size() && star_samples.size()<17;++i) {
          star_samples.emplace_back(center*.75+kernel[i].Coord()*.25);
          if (star_samples.size()<17) star_samples.emplace_back(center*.5+kernel[i].Coord()*.5);
        }
        const double weights[7][3]={{1,0,0},{0,1,0},{0,0,1},{.5,.5,0},{0,.5,.5},{.5,0,.5},{1.0/3,1.0/3,1.0/3}};
        gp_Pnt2d uv; double best_angle=std::numeric_limits<double>::infinity(),best_error=best_angle;
        double rejected_angle=best_angle,rejected_error=best_angle; int examined=0,rejected_child=-1,rejected_sample=-1;
        gp_Pnt2d rejected_uv;
        for (const auto& proposed : star_samples) {
          ++examined;
          if (!strip_finite(proposed) || BRepClass_FaceClassifier(face->GetFace(),proposed,Precision::PConfusion()).State()!=TopAbs_IN) continue;
          const auto proposed_point=face->GetSurface()->Value(proposed.X(),proposed.Y());
          if (!strip_finite(proposed_point)) continue;
          double max_angle=0.0,max_error=0.0; bool valid=true; int worst_child=-1,worst_sample=-1;
          gp_Pnt2d worst_uv;
          for (std::size_t i=0;i<boundary.size() && valid;++i) {
            const gp_Pnt2d u[3]={mesh->UVNode(boundary[i]),mesh->UVNode(boundary[(i+1)%boundary.size()]),proposed};
            const gp_Pnt p[3]={mesh->Node(boundary[i]).Transformed(location.Transformation()),
                mesh->Node(boundary[(i+1)%boundary.size()]).Transformed(location.Transformation()),proposed_point};
            const double area=(u[1].Coord()-u[0].Coord()).Crossed(u[2].Coord()-u[0].Coord());
            const auto normal=gp_Vec(p[0],p[1]).Crossed(gp_Vec(p[0],p[2]));
            if (!std::isfinite(area) || area<=0.0 || !std::isfinite(normal.SquareMagnitude()) || normal.SquareMagnitude()<=0.0) { valid=false; break; }
            int sample_index=0;
            for (const auto& w : weights) {
              if (++work>2097152) throw std::runtime_error("star precision search work budget");
              const gp_Pnt2d sample(u[0].Coord()*w[0]+u[1].Coord()*w[1]+u[2].Coord()*w[2]);
              const gp_Pnt affine(p[0].XYZ()*w[0]+p[1].XYZ()*w[1]+p[2].XYZ()*w[2]);
              gp_Pnt on_surface; gp_Dir source_normal;
              if (!BRepMesh_GeomTool::Normal(face->GetSurface(),sample.X(),sample.Y(),on_surface,source_normal) || !strip_finite(on_surface)) { valid=false; break; }
              const double error=affine.Distance(on_surface),angular=normal.Angle(gp_Vec(source_normal));
              if (!std::isfinite(error) || !std::isfinite(angular)) { valid=false; break; }
              if (angular>max_angle) { max_angle=angular; worst_child=static_cast<int>(i); worst_sample=sample_index; worst_uv=sample; }
              max_error=std::max(max_error,error); ++sample_index;
            }
          }
          if (!valid) continue;
          if (max_angle<rejected_angle) {
            rejected_angle=max_angle; rejected_error=max_error; rejected_child=worst_child;
            rejected_sample=worst_sample; rejected_uv=worst_uv;
          }
          if (max_angle<=angle && max_error<=d && max_angle<best_angle) {
            uv=proposed; best_angle=max_angle; best_error=max_error;
          }
        }
        stage="star search patch/candidates "+std::to_string(patch.size())+"/"+std::to_string(examined);
        if (!std::isfinite(best_angle)) throw std::runtime_error("no precise source star; best D/angle "+
            std::to_string(rejected_error)+"/"+std::to_string(rejected_angle)+" child/sample "+
            std::to_string(rejected_child)+"/"+std::to_string(rejected_sample)+" UV "+
            std::to_string(rejected_uv.X())+"/"+std::to_string(rejected_uv.Y()));
        const auto point=face->GetSurface()->Value(uv.X(),uv.Y());
        if (!strip_finite(point)) throw std::runtime_error("star surface sample is nonfinite");
        const auto replacement=mesh->Copy(); const int node=mesh->NbNodes()+1;
        replacement->ResizeNodes(node,true); replacement->SetUVNode(node,uv);
        replacement->SetNode(node,point.Transformed(location.Transformation().Inverted()));
        for (std::size_t i=0;i<boundary.size();++i) children.push_back({boundary[i],boundary[(i+1)%boundary.size()],node});
        stage="qualifying source star children patch/cells "+std::to_string(patch.size())+"/"+std::to_string(children.size())+
            " search D/angle "+std::to_string(best_error)+"/"+std::to_string(best_angle);
        for (const auto& child : children) {
          gp_Pnt p[3]; gp_Pnt2d u[3];
          for (int i=0;i<3;++i) {
            p[i]=replacement->Node(child[i]).Transformed(location.Transformation()); u[i]=replacement->UVNode(child[i]);
            if (!strip_finite(p[i]) || !strip_finite(u[i])) throw std::runtime_error("nonfinite native child");
            const auto key=link(child[i],child[(i+1)%3]); ++new_links[key]; new_directions[key]+=child[i]<child[(i+1)%3] ? 1 : -1;
            area_scale+=std::pow(std::abs(u[i].X())+std::abs(u[i].Y()),2);
          }
          const double area=.5*(u[1].Coord()-u[0].Coord()).Crossed(u[2].Coord()-u[0].Coord());
          const auto normal=gp_Vec(p[0],p[1]).Crossed(gp_Vec(p[0],p[2]));
          if (!std::isfinite(area) || area<=0.0 || !std::isfinite(normal.SquareMagnitude()) || normal.SquareMagnitude()<=0.0)
            throw std::runtime_error("internal child has zero/inverted native or UV area");
          new_area+=area;
          for (const auto& w : weights) {
            const gp_Pnt2d sample(u[0].Coord()*w[0]+u[1].Coord()*w[1]+u[2].Coord()*w[2]);
            const gp_Pnt affine(p[0].XYZ()*w[0]+p[1].XYZ()*w[1]+p[2].XYZ()*w[2]);
            gp_Pnt on_surface; gp_Dir source_normal;
            if (!BRepMesh_GeomTool::Normal(face->GetSurface(),sample.X(),sample.Y(),on_surface,source_normal) || !strip_finite(on_surface))
              throw std::runtime_error("undefined child source normal");
            const double error=affine.Distance(on_surface),angular=normal.Angle(gp_Vec(source_normal));
            if (!std::isfinite(error) || error>d || !std::isfinite(angular) || angular>angle)
              throw std::runtime_error("child source D/angle "+std::to_string(error)+"/"+std::to_string(angular));
          }
        }
        stage="proving identical oriented UV patch";
        if (!std::isfinite(area_scale) || std::abs(old_area-new_area)>256*std::numeric_limits<double>::epsilon()*area_scale)
          throw std::runtime_error("internal patch area changed");
        for (const auto& entry : old_links) if (entry.second==1) {
          if (entry.second!=1 || new_links[entry.first]!=1 || new_directions[entry.first]!=old_directions[entry.first])
            throw std::runtime_error("internal patch outer links changed");
        }
        for (const auto& entry : new_links) if (!old_links.count(entry.first)) {
          if (entry.second!=2 || new_directions[entry.first]!=0) throw std::runtime_error("child internal incidence is not opposite-two");
        }
        if (new_links.count(link(a,b))) throw std::runtime_error("original shared internal diagonal remains");
        std::vector<std::array<int,3>> result;
        for (int ti=1;ti<=mesh->NbTriangles();++ti) {
          if (patch.count(ti)) continue;
          int ids[3]; mesh->Triangle(ti).Get(ids[0],ids[1],ids[2]); result.push_back({ids[0],ids[1],ids[2]});
        }
        result.insert(result.end(),children.begin(),children.end());
        replacement->ResizeTriangles(static_cast<int>(result.size()),false);
        for (std::size_t ti=0;ti<result.size();++ti) replacement->SetTriangle(static_cast<int>(ti)+1,
            Poly_Triangle(result[ti][0],result[ti][1],result[ti][2]));
        replacement->RemoveNormals(); replacement->ComputeNormals();
        BRep_Builder().UpdateFace(face->GetFace(),replacement);
        stage="full native face domain/incidence";
        accepted=validate_spherical_strip(trial,true);
        if (!accepted) stage += ": "+strip_stop_;
      } catch (const Standard_Failure&) { stage += ": OCCT exception"; }
        catch (const std::exception& e) { stage += ": "+std::string(e.what()).substr(0,180); }
      if (!accepted) {
        try { BRep_Builder().UpdateFace(face->GetFace(),mesh); }
        catch (...) { throw RollbackFailure("OCCT could not restore internal export diagonal"); }
        why="internal edge "+std::to_string(native_edges.FindIndex(candidate.edge->GetEdge())-1)+" slots "+
            std::to_string(candidate.from)+"/"+std::to_string(candidate.to)+" "+stage;
      } else {
        ++repaired; cache.erase(face);
        why="native internal diagonal refined and full domain certified";
      }
    }
    export_boundary_stop_ += "; internal diagonal attempts/repaired "+std::to_string(attempts)+"/"+std::to_string(repaired);
    } catch (const RollbackFailure&) { throw; }
      catch (const Standard_Failure&) {
        export_boundary_stop_ += "; internal diagonal attempts/repaired "+std::to_string(attempts)+"/"+
            std::to_string(repaired)+" precheck OCCT exception";
      } catch (const std::exception& e) {
        export_boundary_stop_ += "; internal diagonal attempts/repaired "+std::to_string(attempts)+"/"+
            std::to_string(repaired)+" precheck "+std::string(e.what()).substr(0,160);
      }
  }

  // Some native triangulators omit collinear UV constraint samples even
  // though their mapped nodes and shared 3D curve samples remain available.
  // Reinsert only those existing nodes into the sole incident triangle.
  bool restore_skipped_strip_nodes(const StripTrial& trial,bool native_export=false,int unused_pole_choice=0,
                                   bool complete_native_wire=false) {
    strip_corner_detail_.clear();
    strip_degenerate_details_.clear();
    try {
      std::size_t local_work=0;
      std::size_t& work=native_export ? export_boundary_work_ : local_work;
      for (const auto& saved : trial.faces) {
        TopLoc_Location location;
        const auto mesh = BRep_Tool::Triangulation(saved.face->GetFace(), location);
        if (mesh.IsNull() || !mesh->HasUVNodes() || mesh->NbTriangles() < 1 ||
            mesh->NbTriangles() > 131072 || mesh->NbNodes() > 65536) {
          strip_stop_ = "shared-node refinement has no bounded triangulation"; return false;
        }
        using Triangle = std::array<int,3>;
        using Link = std::pair<int,int>;
        const auto link = [](int a, int b) { return std::make_pair(std::min(a,b),std::max(a,b)); };
        struct Incidence { int count = 0, triangle = 0; };
        std::map<Link,Incidence> links;
        std::map<int,int> pole_aliases;
        IMeshData::IEdgePtr pole_edge=nullptr;
        if (native_export && !qualify_native_pole(saved,mesh,location,pole_aliases,pole_edge,true,true,unused_pole_choice)) return false;
        const auto canonical=[&](int id) {
          const auto found=pole_aliases.find(id); return found==pole_aliases.end() ? id : found->second;
        };
        std::set<int> used;
        std::vector<Triangle> triangles;
        for (int ti = 1; ti <= mesh->NbTriangles(); ++ti) {
          Triangle ids; mesh->Triangle(ti).Get(ids[0],ids[1],ids[2]);
          for (int id : ids) if (id < 1 || id > mesh->NbNodes()) {
            strip_stop_ = "shared-node refinement has invalid triangle index"; return false;
          }
          if (native_export && certified_zero_pole_cell(mesh,location,ids.data(),pole_aliases)) continue;
          triangles.push_back(ids);
          for (int i = 0; i < 3; ++i) {
            used.insert(ids[i]); auto& entry = links[link(ids[i],ids[(i+1)%3])];
            ++entry.count; entry.triangle = static_cast<int>(triangles.size())-1;
          }
        }
        struct Split { int triangle; std::vector<int> chain; };
        std::map<Link,Split> splits;
        std::set<int> scheduled;
        struct BoundaryPoint { int id; gp_Pnt2d uv; gp_Pnt native; double tolerance; };
        std::vector<std::vector<BoundaryPoint>> wire_chains;
        for (int wi = 0; wi < saved.face->WiresNb(); ++wi) {
          const auto wire = saved.face->GetWire(wi);
          std::vector<BoundaryPoint> chain;
          bool eligible_wire = true;
          for (int ei = 0; ei < wire->EdgesNb(); ++ei) {
            const auto edge = wire->GetEdge(ei);
            const auto orientation = wire->GetEdgeOrientation(ei);
            const auto pc = edge->GetPCurve(saved.face,orientation);
            if (BRep_Tool::Degenerated(edge->GetEdge()) || edge->GetDegenerated()) {
              std::ostringstream detail; detail.precision(11);
              const int face_index = strip_original_faces_.FindIndex(saved.face->GetFace())-1;
              detail << "; face " << face_index << " wire/edge " << wi << '/' << ei << " native/discrete degenerate " <<
                  BRep_Tool::Degenerated(edge->GetEdge()) << '/' << edge->GetDegenerated();
              if (!pc.IsNull() && pc->ParametersNb() >= 2 && pc->ParametersNb() == edge->GetCurve()->ParametersNb()) {
                const int last = pc->ParametersNb()-1;
                const auto first_uv = pc->GetPoint(0), last_uv = pc->GetPoint(last);
                detail << " native/surface separation " << edge->GetCurve()->GetPoint(0).Distance(edge->GetCurve()->GetPoint(last)) << '/' <<
                    saved.face->GetSurface()->Value(first_uv.X(),first_uv.Y()).Distance(
                        saved.face->GetSurface()->Value(last_uv.X(),last_uv.Y()));
                for (int index : {0,pc->ParametersNb()-1}) {
                  const int id = pc->GetIndex(index);
                  const auto uv = pc->GetPoint(index);
                  const auto native = edge->GetCurve()->GetPoint(index);
                  detail << " node " << id << " UV(" << uv.X() << ',' << uv.Y() << ") nativeXYZ(" <<
                      native.X() << ',' << native.Y() << ',' << native.Z() << ')';
                  if (id >= 1 && id <= mesh->NbNodes()) {
                    const auto mapped_uv = mesh->UVNode(id);
                    const auto point = mesh->Node(id).Transformed(location.Transformation());
                    const auto surface = saved.face->GetSurface()->Value(uv.X(),uv.Y());
                    detail << " surfaceXYZ(" << surface.X() << ',' << surface.Y() << ',' << surface.Z() <<
                        ") gap " << native.Distance(surface) << " mapped UV/XYZ gap " << mapped_uv.Distance(uv) << '/' << point.Distance(native);
                  }
                }
              }
              strip_degenerate_details_[face_index] += detail.str().substr(0,650);
            }
            if ((orientation != TopAbs_FORWARD && orientation != TopAbs_REVERSED) || pc.IsNull() ||
                (BRep_Tool::Degenerated(edge->GetEdge()) && (!native_export || !certified_pole_edge(edge,pc,pole_aliases))) ||
                // The separation caller already bounded the entire original
                // wire to 1024 stations; read its unchanged long edges too.
                pc->ParametersNb() < 2 || pc->ParametersNb() > (native_export && complete_native_wire ? 1024 : 256) ||
                pc->ParametersNb() != edge->GetCurve()->ParametersNb() || chain.size()+pc->ParametersNb() > 4096) {
              eligible_wire = false; break;
            }
            const auto next_edge = wire->GetEdge((ei+1)%wire->EdgesNb());
            const auto next_orientation = wire->GetEdgeOrientation((ei+1)%wire->EdgesNb());
            const auto next_pc = next_edge->GetPCurve(saved.face,next_orientation);
            const int last = orientation == TopAbs_REVERSED ? 0 : pc->ParametersNb()-1;
            if (next_pc.IsNull() || next_pc->ParametersNb() < 2) { eligible_wire = false; break; }
            const int next_first = next_orientation == TopAbs_REVERSED ? next_pc->ParametersNb()-1 : 0;
            if (canonical(pc->GetIndex(last)) != canonical(next_pc->GetIndex(next_first)) ||
                pc->GetPoint(last).Distance(next_pc->GetPoint(next_first)) > Precision::PConfusion()) {
              eligible_wire = false; break;
            }
            for (int i = 0; i+1 < pc->ParametersNb(); ++i) {
              const int index = orientation == TopAbs_REVERSED ? pc->ParametersNb()-1-i : i;
              const int original_id = pc->GetIndex(index);
              const int id = canonical(original_id);
              if (id < 1 || id > mesh->NbNodes()) { eligible_wire = false; break; }
              double tolerance = BRep_Tool::Tolerance(edge->GetEdge());
              if (index == 0 || index+1 == pc->ParametersNb()) {
                TopoDS_Vertex first_vertex,last_vertex; TopExp::Vertices(edge->GetEdge(),first_vertex,last_vertex);
                const auto vertex = index == 0 ? first_vertex : last_vertex;
                if (!vertex.IsNull()) tolerance = std::max(tolerance,BRep_Tool::Tolerance(vertex));
              }
              // Keep the original PCurve chart intact for the post-fan pole
              // certificate. Only this separate physical quotient chain uses
              // the representative's mapped UV coordinate.
              const BoundaryPoint point{id,id==original_id ? pc->GetPoint(index) : mesh->UVNode(id),
                  edge->GetCurve()->GetPoint(index),tolerance};
              if (!strip_finite(point.uv) || !strip_finite(point.native) ||
                  !std::isfinite(point.tolerance) || point.tolerance < 0.0) {
                eligible_wire = false; break;
              }
              if (!chain.empty() && chain.back().id == id) {
                if (point.uv.Distance(chain.back().uv) > Precision::PConfusion() ||
                    point.native.Distance(chain.back().native) > Precision::Confusion()) { eligible_wire = false; break; }
              } else chain.push_back(point);
            }
            if (!eligible_wire) break;
          }
          if (native_export && eligible_wire && chain.size()>1 && chain.front().id==chain.back().id) chain.pop_back();
          if (eligible_wire && chain.size() > 2) wire_chains.push_back(std::move(chain));
        }
        bool simple_boundary_proved = false;
        const auto prove_simple_outer = [&]() {
          if (saved.face->WiresNb() != 1 || wire_chains.size() != 1) {
            strip_stop_ = "trim-corner restoration requires one certified nondegenerate outer wire"; return false;
          }
          const auto& boundary = wire_chains.front();
          double area = 0.0;
          const auto origin = mesh->UVNode(boundary.front().id).Coord();
          for (std::size_t i = 0; i < boundary.size(); ++i) {
            const auto a = mesh->UVNode(boundary[i].id), b = mesh->UVNode(boundary[(i+1)%boundary.size()].id);
            if (!strip_finite(a) || !strip_finite(b) || a.Distance(b) <= Precision::PConfusion()) {
              strip_stop_ = "trim-corner outer boundary has unresolved UV segment"; return false;
            }
            area += 0.5*(a.Coord()-origin).Crossed(b.Coord()-origin);
            for (std::size_t j = i+1; j < boundary.size(); ++j) {
              if (++work > 2097152) { strip_stop_ = "trim-boundary simplicity work budget exhausted"; return false; }
              gp_Pnt2d intersection;
              const auto flag = BRepMesh_GeomTool::IntSegSeg(a.Coord(),b.Coord(),mesh->UVNode(boundary[j].id).Coord(),
                  mesh->UVNode(boundary[(j+1)%boundary.size()].id).Coord(),true,true,intersection);
              const bool adjacent = j == i+1 || (i == 0 && j+1 == boundary.size());
              if (flag != BRepMesh_GeomTool::NoIntersection && !(adjacent && flag == BRepMesh_GeomTool::EndPointTouch)) {
                strip_stop_ = "trim-corner outer boundary is not simple: segments " + std::to_string(i) + '/' +
                    std::to_string(j) + " intersection status " + std::to_string(static_cast<int>(flag)); return false;
              }
            }
          }
          if (!std::isfinite(area) || area <= 0.0) { strip_stop_ = "trim-corner outer wire has incorrect winding"; return false; }
          simple_boundary_proved = true; return true;
        };
        for (const auto& chain : wire_chains) {
            std::map<int,int> positions;
            std::set<int> ambiguous;
            std::set<Link> native_links;
            for (std::size_t i=0;i<chain.size();++i) native_links.insert(link(chain[i].id,chain[(i+1)%chain.size()].id));
            for (int i = 0; i < static_cast<int>(chain.size()); ++i)
              if (!positions.emplace(chain[i].id,i).second) ambiguous.insert(chain[i].id);
            for (const auto& entry : links) {
              if (++work > 2097152) { strip_stop_ = "shared-node refinement work budget exhausted"; return false; }
              // A reversed existing boundary link is not a shortcut through
              // the wire's long complement. The final direction guard still
              // rejects incorrect orientation without inventing a fan.
              if (entry.second.count != 1 || native_links.count(entry.first) || !positions.count(entry.first.first) ||
                  !positions.count(entry.first.second) || ambiguous.count(entry.first.first) ||
                  ambiguous.count(entry.first.second)) continue;
              const auto& ids = triangles[entry.second.triangle];
              int cyclic = -1;
              for (int i = 0; i < 3; ++i) if (link(ids[i],ids[(i+1)%3]) == entry.first) cyclic = i;
              if (cyclic < 0) { strip_stop_ = "coarse boundary triangle correspondence failed"; return false; }
              const int from = positions.at(ids[cyclic]), to = positions.at(ids[(cyclic+1)%3]);
              const int distance = (to-from+static_cast<int>(chain.size()))%static_cast<int>(chain.size());
              if (distance <= 1 || distance > 256) continue;
              if (splits.count(entry.first)) { strip_stop_ = "coarse boundary edge has multiple wire chains"; return false; }
              const auto a = mesh->UVNode(ids[cyclic]), b = mesh->UVNode(ids[(cyclic+1)%3]);
              const auto delta = b.Coord()-a.Coord(); const double length2 = delta.SquareModulus();
              if (!strip_finite(a) || !strip_finite(b) || !std::isfinite(length2) || length2 <= 0.0) {
                strip_stop_ = "coarse shared boundary has degenerate UV segment"; return false;
              }
              Split split{entry.second.triangle,{}};
              double previous_fraction = -1.0;
              bool corner = false;
              bool ordered_collinear = true;
              std::string order_rejection;
              for (int offset = 0; offset <= distance; ++offset) {
                const int i = (from+offset)%static_cast<int>(chain.size());
                const int id = chain[i].id; const auto uv = mesh->UVNode(id);
                const auto native_uv = chain[i].uv;
                const double uv_gap = uv.Distance(native_uv);
                const double fraction = (uv.Coord()-a.Coord()).Dot(delta)/length2;
                const double line_gap = std::abs((uv.Coord()-a.Coord()).Crossed(delta))/std::sqrt(length2);
                const double roundoff = 64.0 * std::numeric_limits<double>::epsilon() *
                    (std::abs(a.X())+std::abs(a.Y())+std::abs(b.X())+std::abs(b.Y())+std::sqrt(length2));
                const double correspondence_bound = uv_gap + mesh->UVNode(ids[cyclic]).Distance(chain[from].uv) +
                    mesh->UVNode(ids[(cyclic+1)%3]).Distance(chain[to].uv) + roundoff;
                const auto point = mesh->Node(id).Transformed(location.Transformation());
                const bool ordered=fraction>previous_fraction &&
                    !(offset>0 && offset<distance && (fraction<=0.0 || fraction>=1.0));
                if (!ordered && order_rejection.empty()) {
                  order_rejection="collinear chain node "+std::to_string(id)+" fraction "+std::to_string(fraction)+
                      " is outside ordered chord "+std::to_string(ids[cyclic])+"/"+std::to_string(ids[(cyclic+1)%3]);
                }
                ordered_collinear &= ordered;
                if (!strip_finite(uv) || !strip_finite(native_uv) || !strip_finite(chain[i].native) ||
                    !strip_finite(point) || !std::isfinite(uv_gap) || !std::isfinite(fraction) || !std::isfinite(line_gap) ||
                    uv_gap > Precision::PConfusion() || (!native_export && !ordered) ||
                    (offset > 0 && offset < distance && (used.count(id) || ambiguous.count(id))) ||
                    point.Distance(chain[i].native) > Precision::Confusion()) {
                  std::ostringstream diagnostic; diagnostic.precision(6);
                  const double native_gap=point.Distance(chain[i].native);
                  const char* gate=(!strip_finite(uv) || !strip_finite(native_uv) || !strip_finite(chain[i].native) ||
                      !strip_finite(point) || !std::isfinite(uv_gap) || !std::isfinite(fraction) || !std::isfinite(line_gap)) ? "nonfinite" :
                      uv_gap>Precision::PConfusion() ? "UV-correspondence" : !native_export && !ordered ? "chord-order" :
                      offset>0 && offset<distance && used.count(id) ? "already-used-interior" :
                      offset>0 && offset<distance && ambiguous.count(id) ? "ambiguous-owner" : "native-XYZ-correspondence";
                  diagnostic << "adjacent face " << strip_original_faces_.FindIndex(saved.face->GetFace())-1 <<
                      " gate " << gate << " skipped node " << id << " edge " << ids[cyclic] << '/' << ids[(cyclic+1)%3] <<
                      " third " << ids[(cyclic+2)%3] << " fraction/gap/bound " << fraction << '/' << line_gap << '/' <<
                      correspondence_bound << " UV gap/bound " << uv_gap << '/' << Precision::PConfusion() <<
                      " native XYZ gap/bound " << native_gap << '/' << Precision::Confusion() << " used " << used.count(id);
                  strip_stop_ = diagnostic.str(); return false;
                }
                const auto on_surface = saved.face->GetSurface()->Value(uv.X(),uv.Y());
                const double source_gap = point.Distance(on_surface);
                if (!strip_finite(on_surface) || !std::isfinite(source_gap) ||
                    source_gap > std::min(GetParameters().Deflection,chain[i].tolerance+BRep_Tool::Tolerance(saved.face->GetFace()))) {
                  strip_stop_ = "trim-corner native boundary surface discrepancy exceeds recorded tolerance"; return false;
                }
                corner |= line_gap > correspondence_bound;
                previous_fraction = fraction; split.chain.push_back(id);
              }
              // Separate true native trim corners from collinear subdivision;
              // their new fan requires the complete simple-boundary proof,
              // never an enlarged collinearity tolerance.
              const auto source_a = chain[from].uv, source_b = chain[to].uv;
              const auto source_delta = source_b.Coord()-source_a.Coord();
              if (!std::isfinite(source_delta.SquareModulus()) || source_delta.SquareModulus() <= 0.0) {
                strip_stop_ = "skipped source chain has degenerate UV extent"; return false;
              }
              for (int offset = 1; offset < distance; ++offset) {
                const int i = (from+offset)%static_cast<int>(chain.size());
                const double error = std::abs((chain[i].uv.Coord()-source_a.Coord()).Crossed(source_delta));
                const auto middle = chain[i].uv;
                const double scale = (std::abs(source_a.X())+std::abs(source_a.Y())+std::abs(source_b.X())+
                    std::abs(source_b.Y())+std::abs(middle.X())+std::abs(middle.Y())+source_delta.Modulus())*
                    source_delta.Modulus();
                if (!std::isfinite(error) || !std::isfinite(scale)) {
                  strip_stop_ = "trim-corner source UV scale is nonfinite"; return false;
                }
                if (error > 64.0*std::numeric_limits<double>::epsilon()*scale) {
                  corner = true;
                }
              }
              if (corner) {
                std::ostringstream detail; detail.precision(11);
                detail << "; trim corner face " << strip_original_faces_.FindIndex(saved.face->GetFace())-1 <<
                    " surface type " << static_cast<int>(saved.face->GetSurface()->GetType());
                for (int id : {split.chain.front(),split.chain[1],split.chain.back(),ids[(cyclic+2)%3]}) {
                  const auto uv = mesh->UVNode(id); const auto p = mesh->Node(id).Transformed(location.Transformation());
                  detail << " node " << id << " UV(" << uv.X() << ',' << uv.Y() << ") XYZ(" << p.X() << ',' << p.Y() << ',' << p.Z() << ')';
                }
                strip_corner_detail_ = detail.str();
                if (!simple_boundary_proved && !prove_simple_outer()) return false;
              } else if (!ordered_collinear) {
                // Straight subdivision still requires strict ordered fractions.
                // A genuine trim corner is qualified instead by its complete
                // simple domain and positive, precision-checked children.
                strip_stop_=order_rejection; return false;
              }
              for (std::size_t i = 1; i + 1 < split.chain.size(); ++i)
                if (!scheduled.insert(split.chain[i]).second) {
                  strip_stop_ = "skipped node belongs to multiple boundary chains"; return false;
                }
              splits.emplace(entry.first,std::move(split));
            }
        }
        if (splits.empty() && triangles.size()==static_cast<std::size_t>(mesh->NbTriangles())) continue;
        std::map<int,std::vector<Triangle>> replacements;
        for (const auto& entry : splits) {
          const auto& split = entry.second;
          auto& fan = replacements[split.triangle];
          if (fan.empty()) fan.push_back(triangles[split.triangle]);
          bool found = false;
          for (std::size_t fi = 0; fi < fan.size(); ++fi) {
            const auto old = fan[fi];
            for (int ei = 0; ei < 3; ++ei) {
              if (old[ei] != split.chain.front() || old[(ei+1)%3] != split.chain.back()) continue;
              const int opposite = old[(ei+2)%3];
              fan[fi] = {split.chain[0],split.chain[1],opposite};
              for (std::size_t i = 2; i < split.chain.size(); ++i)
                fan.push_back({split.chain[i-1],split.chain[i],opposite});
              found = true; break;
            }
            if (found) break;
          }
          if (!found) { strip_stop_ = "coarse shared edge lost during fan subdivision"; return false; }
        }
        std::vector<Triangle> result;
        for (std::size_t ti = 0; ti < triangles.size(); ++ti) {
          const auto found = replacements.find(static_cast<int>(ti));
          if (found == replacements.end()) result.push_back(triangles[ti]);
          else result.insert(result.end(),found->second.begin(),found->second.end());
          if (result.size() > 131072) { strip_stop_ = "conforming triangle budget exhausted"; return false; }
        }
        // Qualify each new child on its own source-surface samples, including
        // boundary midpoints omitted by BRepLib's native interior estimator.
        // This is the native sampling contract, not an all-point error proof.
        const double deflection = GetParameters().Deflection;
        const double angular = GetParameters().AngleInterior > 0.0 ?
            GetParameters().AngleInterior : GetParameters().Angle;
        if (!std::isfinite(deflection) || deflection <= 0.0 || !std::isfinite(angular) || angular <= 0.0) {
          strip_stop_ = "restored trim fan has invalid precision request"; return false;
        }
        for (const auto& replacement : replacements) for (const auto& child : replacement.second) {
          gp_Pnt point[3]; gp_Pnt2d uv[3]; gp_Dir normals[3]; bool have_normal[3];
          for (int i = 0; i < 3; ++i) {
            point[i] = mesh->Node(child[i]).Transformed(location.Transformation()); uv[i] = mesh->UVNode(child[i]);
            gp_Pnt normal_point;
            have_normal[i] = BRepMesh_GeomTool::Normal(saved.face->GetSurface(),uv[i].X(),uv[i].Y(),normal_point,normals[i]);
            if (!have_normal[i] || !std::isfinite(normals[i].X()) || !std::isfinite(normals[i].Y()) ||
                !std::isfinite(normals[i].Z())) {
              strip_stop_ = "restored trim fan source normal is undefined at node " + std::to_string(child[i]); return false;
            }
          }
          const double signed_area = (uv[1].Coord()-uv[0].Coord()).Crossed(uv[2].Coord()-uv[0].Coord());
          if (!std::isfinite(signed_area) || signed_area <= 0.0) {
            strip_stop_ = "restored trim fan has inverted or degenerate UV child"; return false;
          }
          if (native_export) {
            const auto facet_normal=gp_Vec(point[0],point[1]).Crossed(gp_Vec(point[0],point[2]));
            if (!std::isfinite(facet_normal.SquareMagnitude()) || facet_normal.SquareMagnitude()<=0.0) {
              strip_stop_="restored export trim fan has zero/nonfinite native area"; return false;
            }
            const double weights[7][3]={{1,0,0},{0,1,0},{0,0,1},{.5,.5,0},{0,.5,.5},{.5,0,.5},{1.0/3,1.0/3,1.0/3}};
            int sample_index=0;
            for (const auto& w : weights) {
              const gp_Pnt2d sample(uv[0].Coord()*w[0]+uv[1].Coord()*w[1]+uv[2].Coord()*w[2]);
              gp_Pnt on_surface; gp_Dir source_normal;
              if (!BRepMesh_GeomTool::Normal(saved.face->GetSurface(),sample.X(),sample.Y(),on_surface,source_normal)) {
                strip_stop_="restored export fan source normal is undefined"; return false;
              }
              const double angle=facet_normal.Angle(gp_Vec(source_normal));
              const gp_Pnt affine(point[0].XYZ()*w[0]+point[1].XYZ()*w[1]+point[2].XYZ()*w[2]);
              const double error=affine.Distance(on_surface);
              if (!strip_finite(on_surface) || !strip_finite(affine) || !std::isfinite(error) || error>deflection) {
                strip_stop_="restored export fan source deflection "+std::to_string(error)+" exceeds "+std::to_string(deflection);
                return false;
              }
              if (!std::isfinite(angle) || angle>angular) {
                std::ostringstream diagnostic; diagnostic.precision(8);
                diagnostic << "export child " << child[0] << '/' << child[1] << '/' << child[2] << " sample " << sample_index <<
                    " source angle/budget " << angle << '/' << angular << " gap " << error <<
                    " UV " << sample.X() << '/' << sample.Y();
                strip_stop_=diagnostic.str();
                return false;
              }
              ++sample_index;
            }
          }
          for (int i = 0; i < 3; ++i) {
            const int next = (i+1)%3;
            const gp_Pnt2d middle((uv[i].Coord()+uv[next].Coord())/2.0);
            const gp_Pnt affine((point[i].XYZ()+point[next].XYZ())/2.0);
            const auto source = saved.face->GetSurface()->Value(middle.X(),middle.Y());
            const double deviation = affine.Distance(source);
            if (!strip_finite(source) || !std::isfinite(deviation) || deviation > deflection) {
              strip_stop_ = "restored trim fan edge deflection " + std::to_string(deviation) +
                  " exceeds " + std::to_string(deflection); return false;
            }
            if (have_normal[i] && have_normal[next]) {
              const double angle = normals[i].Angle(normals[next]);
              if (!std::isfinite(angle) || angle > angular) {
                strip_stop_ = "restored trim fan source normal angle " + std::to_string(angle) +
                    " exceeds " + std::to_string(angular); return false;
              }
            }
          }
          const gp_Pnt2d middle((uv[0].Coord()+uv[1].Coord()+uv[2].Coord())/3.0);
          const gp_Pnt affine((point[0].XYZ()+point[1].XYZ()+point[2].XYZ())/3.0);
          const auto source = saved.face->GetSurface()->Value(middle.X(),middle.Y());
          const double deviation = affine.Distance(source);
          if (!strip_finite(source) || !std::isfinite(deviation) || deviation > deflection) {
            strip_stop_ = "restored trim fan centroid deflection " + std::to_string(deviation) +
                " exceeds " + std::to_string(deflection); return false;
          }
        }
        // Copy before changing connectivity so rollback handles never alias
        // the trial. All node coordinates, UVs and mapped indices are retained.
        const auto repaired = mesh->Copy();
        repaired->ResizeTriangles(static_cast<int>(result.size()), false);
        for (std::size_t ti = 0; ti < result.size(); ++ti)
          repaired->SetTriangle(static_cast<int>(ti)+1,Poly_Triangle(result[ti][0],result[ti][1],result[ti][2]));
        repaired->RemoveNormals(); repaired->ComputeNormals();
        BRep_Builder().UpdateFace(saved.face->GetFace(),repaired);
        BRepLib::UpdateDeflection(saved.face->GetFace());
        if (!std::isfinite(repaired->Deflection()) || repaired->Deflection() > deflection) {
          strip_stop_ = "restored face native deflection " + std::to_string(repaired->Deflection()) +
              " exceeds " + std::to_string(deflection); return false;
        }
      }
      return true;
    } catch (const Standard_Failure&) { strip_stop_ = "OCCT exception refining shared boundary nodes"; return false; }
      catch (const std::exception&) { strip_stop_ = "exception refining shared boundary nodes"; return false; }
  }

  // A native degenerate edge can use distinct UV representatives of one
  // physical vertex. Certify this particular quotient and its omitted chart
  // wedge at native sampling precision; never merge merely short edges.
  static bool certified_pole_edge(IMeshData::IEdgePtr edge,const IMeshData::IPCurveHandle& pc,
                                  const std::map<int,int>& aliases) {
    if (!BRep_Tool::Degenerated(edge->GetEdge()) || pc.IsNull() || pc->ParametersNb()!=2) return false;
    const int a=pc->GetIndex(0),b=pc->GetIndex(1);
    const auto first=aliases.find(a),second=aliases.find(b);
    return (first!=aliases.end() && first->second==b) || (second!=aliases.end() && second->second==a);
  }

  bool qualify_native_pole(const StripFace& saved,const Handle(Poly_Triangulation)& mesh,
                          const TopLoc_Location& location,std::map<int,int>& aliases,
                          IMeshData::IEdgePtr& pole_edge,bool native_export=false,bool identity_only=false,int unused_choice=0) {
    try {
      struct Pole { IMeshData::IEdgePtr edge;IMeshData::IPCurveHandle pc;TopoDS_Vertex vertex;int first,last; };
      std::vector<Pole> poles;
      for (int wi=0;wi<saved.face->WiresNb();++wi) {
        const auto wire=saved.face->GetWire(wi);
        for (int ei=0;ei<wire->EdgesNb();++ei) {
          const auto edge=wire->GetEdge(ei);
          if (BRep_Tool::Degenerated(edge->GetEdge())) poles.push_back({edge,edge->GetPCurve(saved.face,wire->GetEdgeOrientation(ei)),{},0,0});
        }
      }
      if (!native_export || poles.size()<2) return qualify_native_pole_group(saved,mesh,location,aliases,pole_edge,
          native_export,identity_only,unused_choice);
      if (poles.size()!=2 || saved.face->WiresNb()!=1 || mesh.IsNull() || !mesh->HasUVNodes()) {
        strip_stop_="multiple pole composition requires two independent poles in one chart";return false;
      }
      std::map<int,int> identity_pairs;std::set<int> pair_nodes;
      for (auto& pole : poles) {
        TopoDS_Vertex last;TopExp::Vertices(pole.edge->GetEdge(),pole.vertex,last);
        if (pole.vertex.IsNull() || last.IsNull() || !pole.vertex.IsSame(last) || pole.pc.IsNull() ||
            pole.pc->ParametersNb()!=2 || pole.edge->GetCurve()->ParametersNb()!=2) {
          strip_stop_="multiple pole lacks exact native vertex/sample identity";return false;
        }
        pole.first=pole.pc->GetIndex(0);pole.last=pole.pc->GetIndex(1);
        for (int id : {pole.first,pole.last}) if (id<1 || id>mesh->NbNodes() || !pair_nodes.insert(id).second) {
          strip_stop_="multiple pole mapped pairs overlap or lose indices";return false;
        }
        const auto a=pole.edge->GetCurve()->GetPoint(0),b=pole.edge->GetCurve()->GetPoint(1);
        const auto x=mesh->Node(pole.first).Transformed(location.Transformation()),y=mesh->Node(pole.last).Transformed(location.Transformation());
        if (!strip_finite(a) || !strip_finite(b) || !strip_finite(x) || !strip_finite(y) || a.Distance(b)!=0.0 ||
            x.Distance(y)!=0.0 || x.Distance(a)>Precision::Confusion() || y.Distance(b)>Precision::Confusion()) {
          strip_stop_="multiple pole native/mapped world points do not coincide";return false;
        }
        identity_pairs[pole.last]=pole.first;
      }
      if (poles[0].vertex.IsSame(poles[1].vertex)) { strip_stop_="multiple poles share a native vertex graph";return false; }
      std::set<int> used;
      for (int ti=1;ti<=mesh->NbTriangles();++ti) {
        if (++pole_certificate_work_>2097152) { strip_stop_="multiple pole identity work budget";return false; }
        int ids[3];mesh->Triangle(ti).Get(ids[0],ids[1],ids[2]);
        for (int id : ids) if (id<1 || id>mesh->NbNodes()) { strip_stop_="multiple pole triangle index";return false; }
        if (certified_zero_pole_cell(mesh,location,ids,identity_pairs)) continue;
        for (int id : ids) used.insert(id);
      }
      int choice_bit=0;aliases.clear();pole_edge=poles.front().edge;
      for (const auto& pole : poles) {
        const bool unused=!used.count(pole.first) && !used.count(pole.last);
        std::map<int,int> group;IMeshData::IEdgePtr selected=nullptr;
        if (!qualify_native_pole_group(saved,mesh,location,group,selected,true,true,
            unused ? ((unused_choice>>choice_bit++)&1) : 0,pole.edge,&identity_pairs)) return false;
        aliases.insert(group.begin(),group.end());
      }
      if (identity_only) return true;
      for (const auto& pole : poles) {
        std::map<int,int> group;IMeshData::IEdgePtr selected=nullptr;
        if (!qualify_native_pole_group(saved,mesh,location,group,selected,true,false,0,pole.edge,&aliases)) return false;
      }
      return true;
    } catch (const Standard_Failure&) { strip_stop_="OCCT exception composing native pole certificates";return false; }
      catch (const std::exception&) { strip_stop_="exception composing native pole certificates";return false; }
  }

  bool qualify_native_pole_group(const StripFace& saved, const Handle(Poly_Triangulation)& mesh,
                           const TopLoc_Location& location, std::map<int,int>& aliases,
                           IMeshData::IEdgePtr& pole_edge, bool native_export=false, bool identity_only=false,int unused_choice=0,
                           IMeshData::IEdgePtr selected=nullptr,const std::map<int,int>* composition=nullptr) {
    pole_edge = nullptr;
    try {
      IMeshData::IPCurveHandle pole_pc;
      for (int wi = 0; wi < saved.face->WiresNb(); ++wi) {
        const auto wire = saved.face->GetWire(wi);
        for (int ei = 0; ei < wire->EdgesNb(); ++ei) {
          const auto edge = wire->GetEdge(ei);
          if (!BRep_Tool::Degenerated(edge->GetEdge())) continue;
          if (selected && selected!=edge) continue;
          if (pole_edge) { strip_stop_ = "multiple native poles exceed local certificate scope"; return false; }
          pole_edge = edge; pole_pc = edge->GetPCurve(saved.face,wire->GetEdgeOrientation(ei));
        }
      }
      if (!pole_edge) return true;
      if (saved.face->WiresNb() != 1 || pole_pc.IsNull() || pole_pc->ParametersNb() != 2 ||
          pole_edge->GetCurve()->ParametersNb() != 2) {
        strip_stop_ = "native pole requires one outer wire and two endpoint samples"; return false;
      }
      const double deflection = GetParameters().Deflection, budget = deflection/4.0;
      const double angular = GetParameters().AngleInterior > 0.0 ? GetParameters().AngleInterior : GetParameters().Angle;
      double tolerance = BRep_Tool::Tolerance(pole_edge->GetEdge());
      TopoDS_Vertex first,last; TopExp::Vertices(pole_edge->GetEdge(),first,last);
      if (native_export && (first.IsNull() || last.IsNull() || !first.IsSame(last))) {
        strip_stop_ = "export pole lacks one exact native vertex identity"; return false;
      }
      if (!first.IsNull()) tolerance = std::max(tolerance,BRep_Tool::Tolerance(first));
      if (!last.IsNull()) tolerance = std::max(tolerance,BRep_Tool::Tolerance(last));
      tolerance += BRep_Tool::Tolerance(saved.face->GetFace());
      if (!std::isfinite(budget) || budget <= 0.0 || !std::isfinite(angular) || angular <= 0.0 ||
          !std::isfinite(tolerance) || tolerance < 0.0) { strip_stop_ = "native pole has invalid precision/tolerance"; return false; }
      const int id0 = pole_pc->GetIndex(0), id1 = pole_pc->GetIndex(1);
      if (id0 < 1 || id1 < 1 || id0 > mesh->NbNodes() || id1 > mesh->NbNodes() || id0 == id1) {
        strip_stop_ = "native pole mapped endpoints are unsupported"; return false;
      }
      const auto native0 = pole_edge->GetCurve()->GetPoint(0), native1 = pole_edge->GetCurve()->GetPoint(1);
      const auto world = [&](int id) { return mesh->Node(id).Transformed(location.Transformation()); };
      const auto as_float = [native_export](const gp_Pnt& p) {
        return native_export ? p : gp_Pnt(static_cast<float>(p.X()),static_cast<float>(p.Y()),static_cast<float>(p.Z()));
      };
      const auto point0 = world(id0), point1 = world(id1);
      if (!strip_finite(native0) || !strip_finite(native1) || !strip_finite(point0) || !strip_finite(point1) ||
          native0.Distance(native1) > Precision::Confusion() || point0.Distance(point1) > Precision::Confusion() ||
          point0.Distance(native0) > Precision::Confusion() || point1.Distance(native1) > Precision::Confusion() ||
          (native_export && native0.Distance(native1)!=0.0) ||
          as_float(point0).Distance(as_float(point1)) != 0.0) {
        strip_stop_ = "native pole endpoints do not coincide in double and float geometry"; return false;
      }
      std::set<int> used;
      int zero_pole_cells=0;
      for (int ti = 1; ti <= mesh->NbTriangles(); ++ti) {
        if (native_export && ++pole_certificate_work_>2097152) {
          strip_stop_ = "export pole identity inspection budget exhausted"; return false;
        }
        int ids[3]; mesh->Triangle(ti).Get(ids[0],ids[1],ids[2]);
        for (int id : ids) {
          if (id < 1 || id > mesh->NbNodes()) { strip_stop_ = "native pole incident triangle has invalid index"; return false; }
        }
        // Exact-zero physical cells can retain both UV representatives of
        // this native vertex. They have no physical facet; their source UV
        // image still belongs to the separately certified signed wedge.
        if (native_export) {
          bool contains_pair = std::find(ids,ids+3,id0)!=ids+3 && std::find(ids,ids+3,id1)!=ids+3;
          if (composition) for (const auto& pair : *composition)
            contains_pair |= std::find(ids,ids+3,pair.first)!=ids+3 && std::find(ids,ids+3,pair.second)!=ids+3;
          const auto normal=gp_Vec(world(ids[0]),world(ids[1])).Crossed(gp_Vec(world(ids[0]),world(ids[2])));
          if (!std::isfinite(normal.SquareMagnitude())) {
            strip_stop_ = "export pole incident cell has nonfinite physical area"; return false;
          }
          if (contains_pair && normal.SquareMagnitude()==0.0) { ++zero_pole_cells; continue; }
        }
        for (int id : ids) used.insert(id);
      }
      const bool proposing_unused=native_export && identity_only && !used.count(id0) && !used.count(id1);
      if (used.count(id0)+used.count(id1) != 1 && !proposing_unused) {
        strip_stop_ = "native pole requires one physical representative; nodes "+std::to_string(id0)+"/"+
            std::to_string(id1)+" positive-use "+std::to_string(used.count(id0))+"/"+std::to_string(used.count(id1))+
            " certified zero-pair cells "+std::to_string(zero_pole_cells); return false;
      }
      const int representative = proposing_unused ? (unused_choice ? id1 : id0) : used.count(id0) ? id0 : id1;
      const int omitted = representative == id0 ? id1 : id0;
      aliases[omitted] = representative;
      // This only proposes connectivity on a disposable export mesh. The
      // complete chart/wedge/precision certificate is repeated after repair.
      if (identity_only) return true;
      const auto& all_aliases=composition ? *composition : aliases;
      const auto own_alias=all_aliases.find(omitted);
      if (own_alias==all_aliases.end() || own_alias->second!=representative) {
        strip_stop_="native pole composition disagrees with physical representative";return false;
      }
      const auto canonical = [&](int id) { const auto found=all_aliases.find(id);return found==all_aliases.end() ? id : found->second; };
      std::vector<int> original, quotient;
      const auto wire = saved.face->GetWire(0);
      for (int ei = 0; ei < wire->EdgesNb(); ++ei) {
        const auto orientation = wire->GetEdgeOrientation(ei);
        const auto pc = wire->GetEdge(ei)->GetPCurve(saved.face,orientation);
        if ((orientation != TopAbs_FORWARD && orientation != TopAbs_REVERSED) || pc.IsNull() ||
            pc->ParametersNb() < 2 || original.size()+pc->ParametersNb() > 1024) {
          strip_stop_ = "native pole boundary exceeds unique oriented chart scope"; return false;
        }
        for (int i = 0; i+1 < pc->ParametersNb(); ++i) {
          const int index = orientation == TopAbs_REVERSED ? pc->ParametersNb()-1-i : i;
          const int id = pc->GetIndex(index);
          if (id < 1 || id > mesh->NbNodes() || !strip_finite(pc->GetPoint(index)) || !strip_finite(mesh->UVNode(id)) ||
              mesh->UVNode(id).Distance(pc->GetPoint(index)) > Precision::PConfusion()) {
            strip_stop_ = "native pole boundary loses source UV correspondence"; return false;
          }
          if (original.empty() || original.back() != id) original.push_back(id);
          const int mapped = canonical(id);
          if (quotient.empty() || quotient.back() != mapped) quotient.push_back(mapped);
        }
      }
      if (quotient.size()>1 && quotient.front()==quotient.back()) quotient.pop_back();
      const auto occurrence = std::find(original.begin(),original.end(),omitted);
      if (occurrence == original.end() || std::count(original.begin(),original.end(),omitted) != 1) {
        strip_stop_ = "native pole omitted UV node has ambiguous boundary ownership"; return false;
      }
      const int oi = static_cast<int>(occurrence-original.begin()), count = static_cast<int>(original.size());
      int a = original[(oi+count-1)%count], b = omitted, c = original[(oi+1)%count];
      if (a != representative && c != representative) {
        strip_stop_ = "native pole representatives are not consecutive on source boundary"; return false;
      }
      const auto auv = mesh->UVNode(a), buv = mesh->UVNode(b), cuv = mesh->UVNode(c);
      int pole_segment = -1;
      for (int i=0;i<count;++i) {
        const int from=original[i],to=original[(i+1)%count];
        if ((from==id0 && to==id1) || (from==id1 && to==id0)) {
          if (pole_segment!=-1) { strip_stop_ = "native pole trace has ambiguous oriented ownership"; return false; }
          pole_segment=i;
        }
      }
      if (pole_segment==-1) { strip_stop_ = "native pole trace is absent from oriented boundary"; return false; }
      int local_crossings=0;
      gp_Pnt2d local_crossing;
      struct Wedge { int omitted,incoming,outgoing;std::array<gp_Pnt2d,3> uv;double area;int crossings=0;gp_Pnt2d crossing; };
      std::vector<Wedge> wedges;double summed_wedges=0.0;
      for (const auto& alias : all_aliases) {
        const auto found=std::find(original.begin(),original.end(),alias.first);
        if (found==original.end() || std::count(original.begin(),original.end(),alias.first)!=1) {
          strip_stop_="composed pole omitted node has ambiguous original ownership";return false;
        }
        const int index=static_cast<int>(found-original.begin());
        const int before=original[(index+count-1)%count],after=original[(index+1)%count];
        if (before!=alias.second && after!=alias.second) { strip_stop_="composed pole pair is not consecutive";return false; }
        for (const auto& other : all_aliases) if (other.first!=alias.first &&
            (before==other.first || before==other.second || after==other.first || after==other.second)) {
          strip_stop_="composed pole wedge touches another pole neighborhood";return false;
        }
        int trace=-1;
        for (int i=0;i<count;++i) if ((original[i]==alias.first && original[(i+1)%count]==alias.second) ||
            (original[i]==alias.second && original[(i+1)%count]==alias.first)) {
          if (trace!=-1) { strip_stop_="composed pole trace ownership is ambiguous";return false; }trace=i;
        }
        if (trace<0) { strip_stop_="composed pole trace is absent";return false; }
        const std::array<gp_Pnt2d,3> points{mesh->UVNode(before),mesh->UVNode(alias.first),mesh->UVNode(after)};
        const double area=.5*(points[1].Coord()-points[0].Coord()).Crossed(points[2].Coord()-points[0].Coord());
        if (!std::isfinite(area)) { strip_stop_="composed pole wedge has nonfinite area";return false; }
        summed_wedges+=area;wedges.push_back({alias.first,(trace+count-1)%count,(trace+1)%count,points,area});
      }
      if (wedges.size()>1) {
        const auto bounds=[](const Wedge& wedge) {
          std::array<double,4> box{wedge.uv[0].X(),wedge.uv[0].X(),wedge.uv[0].Y(),wedge.uv[0].Y()};
          for (const auto& uv : wedge.uv) { box[0]=std::min(box[0],uv.X());box[1]=std::max(box[1],uv.X());
            box[2]=std::min(box[2],uv.Y());box[3]=std::max(box[3],uv.Y()); }return box;
        };
        const auto x=bounds(wedges[0]),y=bounds(wedges[1]);
        if (!(x[1]<y[0] || y[1]<x[0] || x[3]<y[2] || y[3]<x[2])) {
          strip_stop_="composed pole UV wedges are not certified disjoint";return false;
        }
      }
      const auto in_witnessed_wedge = [&](const gp_Pnt2d& point,const Wedge& wedge) {
        if (!strip_finite(point)) return false;
        const double twice_area=2.0*wedge.area;
        if (!std::isfinite(twice_area) || twice_area==0.0) return false;
        for (int i=0;i<3;++i) {
          const auto from=wedge.uv[i],to=wedge.uv[(i+1)%3];
          const double cross=(to.Coord()-from.Coord()).Crossed(point.Coord()-from.Coord());
          const double coordinates=std::abs(from.X())+std::abs(from.Y())+std::abs(to.X())+
              std::abs(to.Y())+std::abs(point.X())+std::abs(point.Y());
          const double roundoff=64.0*std::numeric_limits<double>::epsilon()*coordinates*coordinates;
          if (!std::isfinite(cross) || !std::isfinite(roundoff) ||
              (twice_area>0.0 ? cross < -roundoff : cross > roundoff)) return false;
        }
        return true;
      };
      // Removing exactly b changes the oriented boundary by triangle (a,b,c).
      // The quotient must be simple. A sole proper source crossing between
      // the incoming/outgoing segments at this native pole is confined to
      // that same witnessed wedge; all other contacts remain disallowed.
      const auto simple_area = [&](const std::vector<int>& polygon,double& area,const char* chart,bool source_chart) {
        const auto reject = [&](const std::string& reason) {
          strip_stop_ = std::string("native pole ")+chart+" chart "+reason; return false;
        };
        if (polygon.size()<3) return reject("has fewer than three nodes");
        area = 0.0; const auto origin = mesh->UVNode(polygon.front()).Coord();
        for (std::size_t i = 0; i < polygon.size(); ++i) {
          const auto p = mesh->UVNode(polygon[i]), q = mesh->UVNode(polygon[(i+1)%polygon.size()]);
          if (!strip_finite(p) || !strip_finite(q) || p.Distance(q)<=Precision::PConfusion())
            return reject("nonfinite/degenerate segment "+std::to_string(polygon[i])+"/"+
                std::to_string(polygon[(i+1)%polygon.size()]));
          area += 0.5*(p.Coord()-origin).Crossed(q.Coord()-origin);
          for (std::size_t j = i+1; j < polygon.size(); ++j) {
            if (++pole_certificate_work_ > 2097152) return reject("exceeded certificate work budget");
            gp_Pnt2d intersection;
            const auto flag = BRepMesh_GeomTool::IntSegSeg(p.Coord(),q.Coord(),mesh->UVNode(polygon[j]).Coord(),
                mesh->UVNode(polygon[(j+1)%polygon.size()]).Coord(),true,true,intersection);
            const bool adjacent = j==i+1 || (i==0 && j+1==polygon.size());
            if (flag!=BRepMesh_GeomTool::NoIntersection && !(adjacent && flag==BRepMesh_GeomTool::EndPointTouch)) {
              bool allowed=false;
              if (source_chart && flag==BRepMesh_GeomTool::Cross) for (auto& wedge : wedges) {
                const auto pair=std::minmax(wedge.incoming,wedge.outgoing);
                if (static_cast<int>(i)!=pair.first || static_cast<int>(j)!=pair.second || wedge.crossings ||
                    !in_witnessed_wedge(intersection,wedge)) continue;
                // Cross must lie strictly inside both native-adjacent source
                // segments, not describe another endpoint or backtracking.
                bool interior=true;
                for (int index : {wedge.incoming,wedge.outgoing}) {
                  const auto from=mesh->UVNode(original[index]),to=mesh->UVNode(original[(index+1)%count]);
                  const auto delta=to.Coord()-from.Coord();
                  const double length2=delta.SquareModulus();
                  if (!std::isfinite(length2) || length2<=0.0) { interior=false; break; }
                  const double fraction=(intersection.Coord()-from.Coord()).Dot(delta)/length2;
                  interior &= std::isfinite(fraction) && fraction>0.0 && fraction<1.0;
                }
                if (interior) { ++wedge.crossings;wedge.crossing=intersection;allowed=true;break; }
              }
              if (allowed) continue;
              std::ostringstream detail; detail.precision(9);
              detail << "intersection status " << static_cast<int>(flag) << " segments " << polygon[i] << '/' <<
                  polygon[(i+1)%polygon.size()] << " and " << polygon[j] << '/' << polygon[(j+1)%polygon.size()] <<
                  " at " << intersection.X() << '/' << intersection.Y();
              return reject(detail.str());
            }
          }
        }
        if (!std::isfinite(area) || area==0.0) return reject("has nonfinite/zero signed area");
        return true;
      };
      double source_area,quotient_area;
      if (!simple_area(original,source_area,"source",true) || !simple_area(quotient,quotient_area,"quotient",false)) return false;
      for (const auto& wedge : wedges) if (wedge.omitted==omitted) { local_crossings=wedge.crossings;local_crossing=wedge.crossing; }
      if (std::signbit(source_area)!=std::signbit(quotient_area)) {
        strip_stop_ = "native pole quotient reverses the source chart winding"; return false;
      }
      const double chart_winding = std::signbit(quotient_area) ? -1.0 : 1.0;
      const double wedge_area = 0.5*(buv.Coord()-auv.Coord()).Crossed(cuv.Coord()-auv.Coord());
      // Bound subtraction and polygon accumulation using the stored chart's
      // coordinate scale, including rounding before the origin subtraction.
      double area_scale = std::abs(source_area)+std::abs(quotient_area);
      for (const auto& wedge : wedges) area_scale+=std::abs(wedge.area);
      for (int id : original) {
        const auto parameter = mesh->UVNode(id);
        area_scale += std::pow(std::abs(parameter.X())+std::abs(parameter.Y())+
            std::abs(auv.X())+std::abs(auv.Y()),2);
      }
      if (!std::isfinite(wedge_area) || !std::isfinite(summed_wedges) || std::abs(source_area-quotient_area-summed_wedges) >
          256.0*std::numeric_limits<double>::epsilon()*area_scale) {
        strip_stop_ = "native pole chart difference is not exactly the sum of original witnessed wedges"; return false;
      }
      // Find the sole oriented physical boundary triangle spanning that wedge.
      const int ca = canonical(a), cc = canonical(c);
      int incident = 0, ti_found = 0;
      for (int ti = 1; ti <= mesh->NbTriangles(); ++ti) {
        int ids[3]; mesh->Triangle(ti).Get(ids[0],ids[1],ids[2]);
        for (int i = 0; i < 3; ++i) if (ids[i]==ca && ids[(i+1)%3]==cc) { ++incident; ti_found=ti; }
      }
      if (incident!=1) { strip_stop_ = "native pole wedge lacks one matching directed incident triangle"; return false; }
      int ids[3]; mesh->Triangle(ti_found).Get(ids[0],ids[1],ids[2]);
      gp_Pnt p[3],fp[3]; gp_Pnt2d uv[3];
      for (int i = 0; i < 3; ++i) {
        p[i]=world(ids[i]); fp[i]=as_float(p[i]); uv[i]=mesh->UVNode(ids[i]);
        if (!strip_finite(p[i]) || !strip_finite(fp[i]) || !strip_finite(uv[i])) {
          strip_stop_ = "native pole incident geometry is nonfinite"; return false;
        }
      }
      const auto normal = gp_Vec(p[0],p[1]).Crossed(gp_Vec(p[0],p[2]));
      const auto float_normal = gp_Vec(fp[0],fp[1]).Crossed(gp_Vec(fp[0],fp[2]));
      if (!std::isfinite(normal.SquareMagnitude()) || normal.SquareMagnitude()<=0.0 ||
          !std::isfinite(float_normal.SquareMagnitude()) || float_normal.SquareMagnitude()<=0.0) {
        strip_stop_ = "native pole incident geometry is degenerate in double or float"; return false;
      }
      const auto segment_distance = [](const gp_Pnt& value,const gp_Pnt& x,const gp_Pnt& y) {
        const gp_Vec edge(x,y); const double length2=edge.SquareMagnitude();
        if (length2<=0.0) return value.Distance(x);
        const double t=std::clamp(gp_Vec(x,value).Dot(edge)/length2,0.0,1.0);
        return value.Distance(gp_Pnt(x.XYZ()+edge.XYZ()*t));
      };
      const auto triangle_distance = [&](const gp_Pnt& value,const gp_Pnt* triangle) {
        const gp_Vec x(triangle[0],triangle[1]), y(triangle[0],triangle[2]), offset(triangle[0],value);
        const double xx=x.SquareMagnitude(), yy=y.SquareMagnitude(), xy=x.Dot(y), determinant=xx*yy-xy*xy;
        if (determinant>0.0) {
          const double u=(offset.Dot(x)*yy-offset.Dot(y)*xy)/determinant;
          const double v=(offset.Dot(y)*xx-offset.Dot(x)*xy)/determinant;
          if (u>=0.0 && v>=0.0 && u+v<=1.0) return value.Distance(gp_Pnt(triangle[0].XYZ()+x.XYZ()*u+y.XYZ()*v));
        }
        return std::min({segment_distance(value,triangle[0],triangle[1]),segment_distance(value,triangle[1],triangle[2]),
            segment_distance(value,triangle[2],triangle[0])});
      };
      gp_Dir reference_normal; gp_Pnt reference_point;
      const gp_Pnt2d center((uv[0].Coord()+uv[1].Coord()+uv[2].Coord())/3.0);
      if (!BRepMesh_GeomTool::Normal(saved.face->GetSurface(),center.X(),center.Y(),reference_point,reference_normal)) {
        strip_stop_ = "native pole incident source normal is undefined"; return false;
      }
      double max_error=0.0,max_angle=0.0,max_pole_gap=0.0,max_affine_error=0.0;
      std::string sample_stop;
      const auto rejected_sample = [&](const char* stage) {
        std::ostringstream detail; detail.precision(7);
        detail << stage << " " << sample_stop << " error/angle " << max_error << '/' << max_angle <<
            " budgets " << budget << '/' << angular << " CAD tolerance " << tolerance;
        detail << " pole/affine error " << max_pole_gap << '/' << max_affine_error;
        strip_stop_ = detail.str(); return false;
      };
      const auto sample = [&](const gp_Pnt2d& parameter,bool collapsed) {
        sample_stop = "invalid sample or work budget";
        if (++pole_certificate_work_>2097152 || !strip_finite(parameter)) return false;
        const auto source=saved.face->GetSurface()->Value(parameter.X(),parameter.Y());
        if (!strip_finite(source)) return false;
        const double error=std::max(triangle_distance(source,p),triangle_distance(source,fp));
        max_error=std::max(max_error,error);
        sample_stop = "source-to-incident geometry gap";
        if (!std::isfinite(error) || error>budget) return false;
        if (collapsed) {
          max_pole_gap = std::max(max_pole_gap,std::max(source.Distance(native0),source.Distance(as_float(point0))));
          sample_stop = "source-to-pole gap";
          if (source.Distance(native0)>std::min(tolerance,budget) || source.Distance(as_float(point0))>budget) return false;
        }
        gp_Dir source_normal; gp_Pnt source_point;
        sample_stop = "undefined source normal";
        if (!BRepMesh_GeomTool::Normal(saved.face->GetSurface(),parameter.X(),parameter.Y(),source_point,source_normal)) return false;
        // The chart may wind in either direction. Compare the actual facet
        // normals with the source normal oriented by that same certified
        // winding; retaining its sign is independent of physical face reversal.
        const gp_Vec oriented_source_normal=gp_Vec(source_normal)*chart_winding;
        const double angle=std::max({reference_normal.Angle(source_normal),
            normal.Angle(oriented_source_normal),float_normal.Angle(oriented_source_normal)});
        max_angle=std::max(max_angle,angle);
        sample_stop = "actual facet angular budget";
        return std::isfinite(angle) && angle<=angular;
      };
      // Sample both the accepted discrete trace and the exact native PCurve.
      double first_parameter,last_parameter;
      const auto exact_pc=BRep_Tool::CurveOnSurface(pole_edge->GetEdge(),saved.face->GetFace(),first_parameter,last_parameter);
      if (exact_pc.IsNull() || !std::isfinite(first_parameter) || !std::isfinite(last_parameter)) {
        strip_stop_ = "native pole source PCurve or parameter range is missing"; return false;
      }
      for (int i=0;i<=8;++i) {
        const double t=i/8.0;
        const auto trace=gp_Pnt2d(pole_pc->GetPoint(0).Coord()*(1.0-t)+pole_pc->GetPoint(1).Coord()*t);
        if (!sample(trace,true) || !sample(exact_pc->Value(first_parameter*(1.0-t)+last_parameter*t),true)) {
          return rejected_sample("native pole trace");
        }
      }
      // Wedge and actual incident triangle are checked directly; the native
      // estimator intentionally excludes triangles at degenerate boundaries.
      if (local_crossings && !sample(local_crossing,false)) return rejected_sample("native pole local crossing");
      for (int i=0;i<=4;++i) for (int j=0;j<=4-i;++j) {
        const double x=i/4.0,y=j/4.0,z=1.0-x-y;
        if (!sample(gp_Pnt2d(auv.Coord()*x+buv.Coord()*y+cuv.Coord()*z),false)) {
          return rejected_sample("native pole omitted wedge");
        }
        const gp_Pnt2d parameter(uv[0].Coord()*x+uv[1].Coord()*y+uv[2].Coord()*z);
        const auto source=saved.face->GetSurface()->Value(parameter.X(),parameter.Y());
        const gp_Pnt affine(p[0].XYZ()*x+p[1].XYZ()*y+p[2].XYZ()*z), f_affine(fp[0].XYZ()*x+fp[1].XYZ()*y+fp[2].XYZ()*z);
        if (!sample(parameter,false) || !strip_finite(source)) {
          return rejected_sample("native pole incident triangle");
        }
        const double affine_error = std::max(source.Distance(affine),source.Distance(f_affine));
        max_affine_error = std::max(max_affine_error,affine_error);
        if (!std::isfinite(affine_error) || affine_error>deflection) {
          sample_stop = "affine triangle deflection";
          return rejected_sample("native pole incident triangle");
        }
      }
      std::ostringstream certificate; certificate.precision(7);
      certificate << "; certified native pole " << omitted << "->" << representative << " D/angle " << deflection << '/' << angular <<
          " measured error/angle " << max_error << '/' << max_angle << " signed chart wedge " << wedge_area;
      certificate << " localized source crossings " << local_crossings;
      strip_corner_detail_ += certificate.str();
      return true;
    } catch (const Standard_Failure&) { strip_stop_ = "OCCT exception certifying native pole quotient"; return false; }
      catch (const std::exception&) { strip_stop_ = "exception certifying native pole quotient"; return false; }
  }

  bool validate_spherical_strip(const StripTrial& trial,bool native_export=false,bool all_source_witnesses=false) {
    strip_stop_ = "triangulation status";
    try {
      std::map<IMeshData::IEdgePtr, std::pair<int, int>> shared_incidence;
      for (const auto& edge : trial.edges) shared_incidence[edge.edge] = {0,0};
      for (const auto& saved : trial.faces) {
        const auto face = saved.face;
        std::set<int> changed_boundary_nodes;
        if (native_export) for (const auto& shared : trial.edges) for (const auto& pc : shared.pcurves) {
          if (pc.curve->GetFace()!=face) continue;
          for (int i=0;i<pc.curve->ParametersNb();++i) changed_boundary_nodes.insert(pc.curve->GetIndex(i));
        }
        TopLoc_Location location;
        const auto mesh = BRep_Tool::Triangulation(face->GetFace(), location);
        if ((face->GetStatusMask() & ~(IMeshData_Outdated | IMeshData_Reused)) != 0 || mesh.IsNull() ||
            !mesh->HasUVNodes() || mesh->NbNodes() < 3 || mesh->NbNodes() > 65536 ||
            mesh->NbTriangles() < 1 || mesh->NbTriangles() > 131072) return false;
        std::map<int,int> pole_aliases;
        IMeshData::IEdgePtr pole_edge = nullptr;
        if (!qualify_native_pole(saved,mesh,location,pole_aliases,pole_edge,native_export)) {
          strip_stop_ = "adjacent face " + std::to_string(strip_original_faces_.FindIndex(face->GetFace())-1) +
              " " + strip_stop_; return false;
        }
        const auto canonical = [&](int id) {
          const auto alias = pole_aliases.find(id);
          return alias == pole_aliases.end() ? id : alias->second;
        };
        std::map<std::pair<int,int>, int> links;
        std::map<std::pair<int,int>, int> directions, boundary_directions;
        std::map<std::pair<int,int>, std::vector<int>> boundary_orientations;
        std::map<int,std::vector<std::array<int,3>>> boundary_owners;
        std::set<std::pair<int,int>> boundary;
        const auto link = [](int a, int b) { return std::make_pair(std::min(a,b), std::max(a,b)); };
        const auto direction = [](int a, int b) { return a < b ? 1 : -1; };
        double uv_area = 0.0, uv_scale = 0.0, expected_area = 0.0, mapped_area = 0.0, max_uv_gap = 0.0;
        for (int wi = 0; wi < face->WiresNb(); ++wi) {
          const auto& wire = face->GetWire(wi);
          if (wire->GetStatusMask() != 0) return false;
          std::vector<gp_Pnt2d> polygon;
          std::vector<gp_Pnt2d> mapped_polygon;
          for (int ei = 0; ei < wire->EdgesNb(); ++ei) {
            const auto edge = wire->GetEdge(ei);
            const auto pc = edge->GetPCurve(face, wire->GetEdgeOrientation(ei));
            const auto curve = edge->GetCurve();
            if (pc.IsNull() || pc->ParametersNb() != curve->ParametersNb()) return false;
            const int traversal = wire->GetEdgeOrientation(ei) == TopAbs_REVERSED ? -1 : 1;
            if (shared_incidence.count(edge)) {
              if (saved.original_orientation != TopAbs_FORWARD && saved.original_orientation != TopAbs_REVERSED) return false;
              ++shared_incidence[edge].first;
              shared_incidence[edge].second += traversal * (saved.original_orientation == TopAbs_REVERSED ? -1 : 1);
            }
            for (int i = 0; i < pc->ParametersNb(); ++i) {
              const int id = pc->GetIndex(i);
              strip_stop_ = "adjacent shared boundary mismatch";
              if (id < 1 || id > mesh->NbNodes() || mesh->Node(id).Transformed(location.Transformation()).Distance(curve->GetPoint(i)) >
                    Precision::Confusion() || mesh->UVNode(id).Distance(pc->GetPoint(i)) > Precision::PConfusion()) return false;
              max_uv_gap = std::max(max_uv_gap, mesh->UVNode(id).Distance(pc->GetPoint(i)));
              if (boundary_owners[id].size() < 2) boundary_owners[id].push_back({wi,ei,i});
              if (i && id != pc->GetIndex(i-1)) {
                const int previous = canonical(pc->GetIndex(i-1)), current = canonical(id);
                if (previous == current) {
                  // Only a certified native degenerate edge may disappear in
                  // the physical boundary quotient. Other shared edges retain
                  // every original nondegenerate segment.
                  if (!certified_pole_edge(edge,pc,pole_aliases)) {
                    strip_stop_ = "native pole alias collapses a nondegenerate boundary edge"; return false;
                  }
                } else {
                  const auto key = link(previous, current);
                  boundary.insert(key);
                  boundary_directions[key] += traversal * direction(previous, current);
                  if (boundary_orientations[key].size() < 4)
                    boundary_orientations[key].push_back(static_cast<int>(wire->GetEdgeOrientation(ei)));
                }
              }
              if (i + 1 < pc->ParametersNb()) {
                const int index = wire->GetEdgeOrientation(ei) == TopAbs_REVERSED ? pc->ParametersNb()-1-i : i;
                polygon.push_back(pc->GetPoint(index));
                const int mapped_index = pc->GetIndex(index);
                if (mapped_index < 1 || mapped_index > mesh->NbNodes()) return false;
                mapped_polygon.push_back(mesh->UVNode(canonical(mapped_index)));
              }
            }
          }
          if (polygon.size() < 3) return false;
          const auto origin = polygon.front().Coord();
          const auto mapped_origin = mapped_polygon.front().Coord();
          for (std::size_t i = 0; i < polygon.size(); ++i) {
            expected_area += 0.5 * (polygon[i].Coord()-origin).Crossed(polygon[(i+1)%polygon.size()].Coord()-origin);
            mapped_area += 0.5 * (mapped_polygon[i].Coord()-mapped_origin).Crossed(
                mapped_polygon[(i+1)%mapped_polygon.size()].Coord()-mapped_origin);
          }
        }
        for (int ti = 1; ti <= mesh->NbTriangles(); ++ti) {
          int ids[3]; mesh->Triangle(ti).Get(ids[0], ids[1], ids[2]);
          gp_Pnt p[3], fp[3]; gp_Pnt2d uv[3];
          strip_stop_ = "nonfinite or degenerate adjacent triangle";
          for (int i = 0; i < 3; ++i) {
            if (ids[i] < 1 || ids[i] > mesh->NbNodes()) return false;
            // The omitted UV representative was proved unused. Never
            // canonicalize an actual triangle or collapse its physical edge.
            if (canonical(ids[i]) != ids[i]) {
              strip_stop_ = "native pole alias changes an actual triangle node"; return false;
            }
            p[i] = mesh->Node(ids[i]).Transformed(location.Transformation()); uv[i] = mesh->UVNode(ids[i]);
            fp[i] = gp_Pnt(static_cast<float>(p[i].X()), static_cast<float>(p[i].Y()), static_cast<float>(p[i].Z()));
            if (!strip_finite(p[i]) || (!native_export && !strip_finite(fp[i])) || !strip_finite(uv[i])) return false;
            ++links[link(ids[i],ids[(i+1)%3])];
            directions[link(ids[i],ids[(i+1)%3])] += direction(ids[i],ids[(i+1)%3]);
          }
          const auto normal = gp_Vec(p[0],p[1]).Crossed(gp_Vec(p[0],p[2]));
          const auto float_normal = gp_Vec(fp[0],fp[1]).Crossed(gp_Vec(fp[0],fp[2]));
          const double area = 0.5 * (uv[1].Coord()-uv[0].Coord()).Crossed(uv[2].Coord()-uv[0].Coord());
          if (!std::isfinite(normal.SquareMagnitude()) || normal.SquareMagnitude() <= (native_export ? 0.0 : 1e-24) ||
              (!native_export && (!std::isfinite(float_normal.SquareMagnitude()) || float_normal.SquareMagnitude() <= 0.0 ||
              !std::isfinite(float_normal.Dot(normal)) || float_normal.Dot(normal) <= 0.0)) ||
              !std::isfinite(area) || area == 0.0 || std::signbit(area) != std::signbit(expected_area)) return false;
          gp_Pnt surface_point; gp_Vec du, dv;
          const gp_Pnt2d centroid_uv((uv[0].Coord()+uv[1].Coord()+uv[2].Coord())/3.0);
          face->GetSurface()->D1(centroid_uv.X(), centroid_uv.Y(), surface_point, du, dv);
          const auto surface_normal = du.Crossed(dv);
          const double geometric_winding = normal.Dot(surface_normal);
          strip_stop_ = "adjacent geometric normal winding";
          if (!std::isfinite(surface_normal.SquareMagnitude()) || surface_normal.SquareMagnitude() <= 0.0 ||
              !std::isfinite(geometric_winding) || geometric_winding == 0.0 ||
              std::signbit(geometric_winding) != std::signbit(area)) {
            bool changed=saved.triangulation.IsNull() || ti>saved.triangulation->NbTriangles();
            if (!changed) {
              const auto old=saved.triangulation->Triangle(ti);
              for (int i=0;i<3;++i) changed |= old.Value(i+1)!=ids[i];
            }
            std::ostringstream diagnostic; diagnostic.precision(8);
            diagnostic << "face " << strip_original_faces_.FindIndex(face->GetFace())-1 << " tri " << ti <<
                (changed ? " changed" : " original") << " nodes " << ids[0] << '/' << ids[1] << '/' << ids[2] <<
                " winding facet2/D1normal2/dot " << normal.SquareMagnitude() << '/' << surface_normal.SquareMagnitude() << '/' <<
                geometric_winding << " UVarea " << area << " adaptor orientation " << static_cast<int>(face->GetSurface()->Face().Orientation());
            gp_Pnt normalized_point; gp_Dir normalized_normal;
            try {
              if (BRepMesh_GeomTool::Normal(face->GetSurface(),centroid_uv.X(),centroid_uv.Y(),normalized_point,normalized_normal))
                diagnostic << " normalized angle " << normal.Angle(gp_Vec(normalized_normal)*(std::signbit(area) ? -1.0 : 1.0));
              else diagnostic << " normalized source undefined";
            } catch (const Standard_Failure&) { diagnostic << " normalized diagnostic unavailable (OCCT)"; }
              catch (const std::exception&) { diagnostic << " normalized diagnostic unavailable (native)"; }
            diagnostic << " UV " << centroid_uv.X() << '/' << centroid_uv.Y();
            strip_stop_=diagnostic.str(); return false;
          }
          uv_area += std::abs(area);
          double diameter = 0.0;
          for (int i = 0; i < 3; ++i) for (int j = i+1; j < 3; ++j)
            diameter = std::max(diameter, std::abs(uv[i].X()-uv[j].X()) + std::abs(uv[i].Y()-uv[j].Y()));
          uv_scale += diameter * diameter;
          if (native_export && (all_source_witnesses || face==trial.target || changed_boundary_nodes.count(ids[0]) ||
              changed_boundary_nodes.count(ids[1]) || changed_boundary_nodes.count(ids[2]))) {
            const double angular=GetParameters().AngleInterior>0.0 ? GetParameters().AngleInterior : GetParameters().Angle;
            if (!std::isfinite(angular) || angular<=0.0) { strip_stop_="native strip invalid angular request";return false; }
            const double weights[7][3]={{1,0,0},{0,1,0},{0,0,1},{.5,.5,0},{0,.5,.5},{.5,0,.5},{1.0/3,1.0/3,1.0/3}};
            int sample_index=0;
            for (const auto& w : weights) {
              if (++export_boundary_work_>2097152) { strip_stop_="native strip source witness budget";return false; }
              const gp_Pnt2d at(uv[0].Coord()*w[0]+uv[1].Coord()*w[1]+uv[2].Coord()*w[2]);
              const gp_Pnt affine(p[0].XYZ()*w[0]+p[1].XYZ()*w[1]+p[2].XYZ()*w[2]);gp_Pnt source;gp_Dir direction;
              if (!BRepMesh_GeomTool::Normal(face->GetSurface(),at.X(),at.Y(),source,direction) || !strip_finite(source)) {
                strip_stop_="native strip source normal undefined";return false;
              }
              const double distance=affine.Distance(source),error=normal.Angle(gp_Vec(direction)*(std::signbit(area) ? -1.0 : 1.0));
              if (!std::isfinite(distance) || !std::isfinite(error) || distance>GetParameters().Deflection || error>angular) {
                strip_stop_="native strip face/tri/sample "+std::to_string(strip_original_faces_.FindIndex(face->GetFace())-1)+"/"+
                    std::to_string(ti)+"/"+std::to_string(sample_index)+" source D/angle "+std::to_string(distance)+"/"+std::to_string(error);return false;
              }
              ++sample_index;
            }
          }
          if (face == trial.target) {
            strip_stop_ = "spherical radial winding or float precision";
            const auto center = face->GetSurface()->Sphere().Location();
            const gp_Pnt centroid((p[0].XYZ()+p[1].XYZ()+p[2].XYZ())/3.0);
            const gp_Pnt fcentroid((fp[0].XYZ()+fp[1].XYZ()+fp[2].XYZ())/3.0);
            if (normal.Dot(gp_Vec(center,centroid)) <= 0.0 || (!native_export &&
                (!std::isfinite(float_normal.SquareMagnitude()) || float_normal.SquareMagnitude() <= 0.0 ||
                float_normal.Dot(gp_Vec(center,fcentroid)) <= 0.0))) return false;
            double source_error = 0.0;
            for (int i = 0; i < 3; ++i) source_error = std::max(source_error,
                (native_export ? p[i] : fp[i]).Distance(face->GetSurface()->Value(uv[i].X(), uv[i].Y())));
            strip_stop_ = "spherical triangle plus region deflection";
            if (source_error + 0.5 * face->GetSurface()->Sphere().Radius() * diameter * diameter +
                trial.coverage > GetParameters().Deflection) return false;
          }
        }
        const int face_index = strip_original_faces_.FindIndex(face->GetFace()) - 1;
        for (const auto& entry : links) {
          const int expected_count = boundary.count(entry.first) ? 1 : 2;
          const int expected_direction = boundary.count(entry.first) ? boundary_directions[entry.first] : 0;
          if (entry.second != expected_count || directions[entry.first] != expected_direction) {
            std::ostringstream diagnostic;
            diagnostic << "face " << face_index << " link " << entry.first.first << '/' << entry.first.second <<
                " count " << entry.second << '/' << expected_count << " dir " << directions[entry.first] << '/' << expected_direction;
            if (boundary.count(entry.first) == 0) {
              for (int id : {entry.first.first,entry.first.first+1,entry.first.second}) {
                if (id > entry.first.second) continue;
                diagnostic << " node" << id << '=';
                for (const auto& owner : boundary_owners[id]) diagnostic << owner[0] << '/' << owner[1] << '/' << owner[2] << ',';
              }
            } else {
              diagnostic << " orientations";
              for (int orientation : boundary_orientations[entry.first]) diagnostic << ' ' << orientation;
            }
            strip_stop_ = diagnostic.str();
            const auto degenerate = strip_degenerate_details_.find(face_index);
            if (degenerate != strip_degenerate_details_.end()) strip_stop_ += degenerate->second.substr(0,550);
            return false;
          }
        }
        for (const auto& entry : boundary) if (links[entry] != 1) {
          strip_stop_ = "adjacent face " + std::to_string(face_index) + " missing boundary link " +
              std::to_string(entry.first) + '/' + std::to_string(entry.second) + " count " + std::to_string(links[entry]);
          return false;
        }
        const double area_roundoff = 128.0 * std::numeric_limits<double>::epsilon() * uv_scale;
        // Every mapped boundary UV has already passed its independent source
        // PCurve correspondence check. Coverage is of that actual accepted
        // boundary, including legal native UV-node reconciliation.
        if (!std::isfinite(uv_area) || !std::isfinite(expected_area) || !std::isfinite(mapped_area) ||
            std::abs(uv_area-std::abs(mapped_area)) > area_roundoff) {
          std::ostringstream diagnostic; diagnostic.precision(7);
          diagnostic << "adjacent face " << face_index << " UV area delta " << std::abs(uv_area-std::abs(expected_area)) <<
              " bound " << area_roundoff << " mapped delta " << std::abs(uv_area-std::abs(mapped_area)) <<
              " max UV gap " << max_uv_gap;
          strip_stop_ = diagnostic.str(); return false;
        }
      }
      strip_stop_ = "opposite shared-edge face incidence";
      for (const auto& entry : shared_incidence)
        if (entry.second.first != 2 || entry.second.second != 0) return false;
      return true;
    } catch (const Standard_Failure&) { strip_stop_ = "OCCT exception validating regularized neighbors"; return false; }
      catch (const std::exception&) { strip_stop_ = "exception validating regularized neighbors"; return false; }
  }

  bool retry_spherical_face(const IMeshData::IFaceHandle& face,
                            const Message_ProgressRange& range) {
    spherical_retry_stop_ = "eligibility or boundary check";
    struct RollbackFailure : std::runtime_error {
      RollbackFailure() : std::runtime_error("OCCT could not restore a failed spherical mesh retry") {}
    };
    try {
      TopLoc_Location old_location;
      if (!BRep_Tool::Triangulation(face->GetFace(), old_location).IsNull()) return false;
      BRepMesh_FaceChecker checker(face, GetParameters());
      if (!checker.Perform()) return false;
      struct Boundary {
        IMeshData::IPCurveHandle pcurve;
        IMeshData::ICurveHandle curve;
        std::vector<int> indices;
      };
      std::vector<Boundary> boundaries;
      std::vector<IMeshData::IWireHandle> wires;
      double expected_uv_area = 0.0;
      int count = 0;
      for (int wi = 0; wi < face->WiresNb(); ++wi) {
        const auto& wire = face->GetWire(wi);
        if (wire->GetStatusMask() != 0) return false;
        wires.push_back(wire);
        std::vector<gp_Pnt2d> polygon;
        for (int ei = 0; ei < wire->EdgesNb(); ++ei) {
          const auto& edge = wire->GetEdge(ei);
          const auto& pc = edge->GetPCurve(face.get(), wire->GetEdgeOrientation(ei));
          const auto& curve = edge->GetCurve();
          if (pc.IsNull() || pc->ParametersNb() < 2 ||
              pc->ParametersNb() != curve->ParametersNb() ||
              (count += pc->ParametersNb()) > 4096 || boundaries.size() >= 16) return false;
          Boundary boundary{pc, curve, {}};
          for (int i = 0; i < pc->ParametersNb(); ++i)
            boundary.indices.push_back(pc->GetIndex(i));
          boundaries.push_back(std::move(boundary));
          for (int i = 0; i < pc->ParametersNb() - 1; ++i)
            polygon.push_back(pc->GetPoint(wire->GetEdgeOrientation(ei) == TopAbs_REVERSED ?
                pc->ParametersNb() - 1 - i : i));
        }
        if (polygon.size() < 3) return false;
        const auto origin = polygon.front().Coord();
        for (std::size_t i = 0; i < polygon.size(); ++i)
          expected_uv_area += 0.5 * (polygon[i].Coord() - origin).Crossed(
              polygon[(i + 1) % polygon.size()].Coord() - origin);
      }
      if (!std::isfinite(expected_uv_area) || expected_uv_area == 0.0) return false;
      const int old_status = face->GetStatusMask();
      const auto rollback = [&]() {
        face->UnsetStatus(static_cast<IMeshData_Status>(face->GetStatusMask()));
        face->SetStatus(static_cast<IMeshData_Status>(old_status));
        for (const auto& wire : wires)
          wire->UnsetStatus(static_cast<IMeshData_Status>(wire->GetStatusMask()));
        for (const auto& boundary : boundaries)
          for (int i = 0; i < static_cast<int>(boundary.indices.size()); ++i)
            boundary.pcurve->GetIndex(i) = boundary.indices[i];
        try {
          BRep_Builder().UpdateFace(face->GetFace(), Handle(Poly_Triangulation)());
        } catch (const Standard_Failure&) {
          throw RollbackFailure();
        } catch (const std::exception&) {
          throw RollbackFailure();
        }
      };
      try {
        // Retry only the failed face, before ModelPostProcessor attaches shared
        // edge polygons. Both factories consume the identical discrete boundary.
        BRepMesh_DelabellaMeshAlgoFactory factory;
        const auto algorithm = factory.GetAlgo(GeomAbs_Sphere, GetParameters());
        if (algorithm.IsNull()) return false;
        // This flag belongs solely to the failed first attempt. Restore it on
        // every rejected trial; success still requires the full validation below.
        face->UnsetStatus(IMeshData_Failure);
        algorithm->Perform(face, GetParameters(), range);
        TopLoc_Location location;
        const auto triangulation = BRep_Tool::Triangulation(face->GetFace(), location);
        bool valid = !triangulation.IsNull() && triangulation->HasUVNodes() &&
            triangulation->NbNodes() >= 3 && triangulation->NbNodes() <= 8192 &&
            triangulation->NbTriangles() > 0 && triangulation->NbTriangles() <= 16384 &&
            std::isfinite(triangulation->Deflection()) && triangulation->Deflection() >= 0.0 &&
            (face->GetStatusMask() & ~(IMeshData_Outdated | IMeshData_Failure)) == 0;
        spherical_retry_stop_ = "trial triangulation null " + std::to_string(triangulation.IsNull()) + " status " +
            std::to_string(face->GetStatusMask()) + " nodes/triangles " +
            std::to_string(triangulation.IsNull() ? 0 : triangulation->NbNodes()) + '/' +
            std::to_string(triangulation.IsNull() ? 0 : triangulation->NbTriangles());
        const auto finite = [](const gp_Pnt& point) {
          return std::isfinite(point.X()) && std::isfinite(point.Y()) && std::isfinite(point.Z());
        };
        const double deflection = GetParameters().Deflection;
        const double radius = face->GetSurface()->Sphere().Radius();
        valid &= std::isfinite(deflection) && deflection > 0.0 &&
            std::isfinite(radius) && radius > 0.0;
        std::map<std::pair<int, int>, int> links;
        std::set<std::pair<int, int>> boundary_links;
        double actual_uv_area = 0.0, uv_area_scale = 0.0;
        const auto link = [](int a, int b) { return std::make_pair(std::min(a, b), std::max(a, b)); };
        if (valid) {
          for (int i = 1; i <= triangulation->NbNodes() && valid; ++i) {
            spherical_retry_stop_ = "nonfinite node";
            const auto uv = triangulation->UVNode(i);
            valid &= finite(triangulation->Node(i)) && std::isfinite(uv.X()) && std::isfinite(uv.Y());
          }
          for (int ti = 1; ti <= triangulation->NbTriangles() && valid; ++ti) {
            spherical_retry_stop_ = "invalid triangle index";
            int ids[3]; triangulation->Triangle(ti).Get(ids[0], ids[1], ids[2]);
            for (int id : ids) valid &= id >= 1 && id <= triangulation->NbNodes();
            if (!valid || ids[0] == ids[1] || ids[1] == ids[2] || ids[2] == ids[0]) {
              valid = false; break;
            }
            gp_Pnt points[3]; gp_Pnt2d uv[3];
            double source_error = 0.0, diameter = 0.0;
            for (int i = 0; i < 3; ++i) {
              points[i] = triangulation->Node(ids[i]).Transformed(location.Transformation());
              uv[i] = triangulation->UVNode(ids[i]);
              const auto source = face->GetSurface()->Value(uv[i].X(), uv[i].Y());
              if (!finite(points[i]) || !finite(source)) { valid = false; break; }
              source_error = std::max(source_error, points[i].Distance(source));
              ++links[link(ids[i], ids[(i + 1) % 3])];
            }
            if (!valid) break;
            const double area_squared = gp_Vec(points[0], points[1]).Crossed(
                gp_Vec(points[0], points[2])).SquareMagnitude();
            spherical_retry_stop_ = "zero area or incorrect UV winding";
            valid &= std::isfinite(area_squared) && area_squared > 0.0;
            const double triangle_uv_area = 0.5 * (uv[1].Coord() - uv[0].Coord()).Crossed(
                uv[2].Coord() - uv[0].Coord());
            valid &= std::isfinite(triangle_uv_area) && triangle_uv_area != 0.0 &&
                std::signbit(triangle_uv_area) == std::signbit(expected_uv_area);
            actual_uv_area += std::abs(triangle_uv_area);
            for (int i = 0; i < 3; ++i)
              for (int j = i + 1; j < 3; ++j)
                diameter = std::max(diameter, std::abs(uv[i].X() - uv[j].X()) +
                    std::abs(uv[i].Y() - uv[j].Y()));
            uv_area_scale += diameter * diameter;
            // A sphere's directional second derivative is bounded by
            // radius*(|du|+|dv|)^2. This bounds interpolation error across the
            // entire triangle, including its original 3D boundary-node error.
            const double error_bound = source_error + 0.5 * radius * diameter * diameter;
            if (valid) spherical_retry_stop_ = "sphere deflection bound";
            valid &= std::isfinite(error_bound) && error_bound <= deflection;
          }
          for (const auto& boundary : boundaries) {
            for (int i = 0; i < boundary.pcurve->ParametersNb() && valid; ++i) {
              spherical_retry_stop_ = "shared boundary node mismatch";
              const int id = boundary.pcurve->GetIndex(i);
              if (id < 1 || id > triangulation->NbNodes()) { valid = false; break; }
              const auto point = triangulation->Node(id).Transformed(location.Transformation());
              const auto& original = boundary.curve->GetPoint(i);
              // No tolerance-sized replacement of a shared 3D boundary sample.
              valid &= finite(original) && point.Distance(original) <= Precision::Confusion() &&
                  triangulation->UVNode(id).Distance(boundary.pcurve->GetPoint(i)) <= Precision::PConfusion();
              if (i > 0) {
                const int previous = boundary.pcurve->GetIndex(i - 1);
                if (previous == id) {
                  valid &= original.Distance(boundary.curve->GetPoint(i - 1)) <= Precision::Confusion();
                } else boundary_links.insert(link(previous, id));
              }
            }
          }
          if (valid) spherical_retry_stop_ = "incomplete or nonmanifold boundary";
          for (const auto& entry : links)
            valid &= entry.second == (boundary_links.count(entry.first) ? 1 : 2);
          for (const auto& entry : boundary_links)
            valid &= links.count(entry) != 0 && links[entry] == 1;
          const double area_roundoff = 64.0 * std::numeric_limits<double>::epsilon() * uv_area_scale;
          if (valid) spherical_retry_stop_ = "UV domain coverage";
          valid &= std::isfinite(actual_uv_area) && std::isfinite(area_roundoff) &&
              std::abs(actual_uv_area - std::abs(expected_uv_area)) <= area_roundoff;
        }
        if (valid) spherical_retry_stop_ = "wire status or cancelled operation";
        for (const auto& wire : wires) valid &= wire->GetStatusMask() == 0;
        if (!valid || !range.More()) { rollback(); return false; }
        triangulation->Deflection(deflection);
        // The original generic Failure is cleared only after a complete,
        // conforming triangulation exists; other failure bits are never cleared.
        face->UnsetStatus(IMeshData_Failure);
        return true;
      } catch (const RollbackFailure&) {
        throw;
      } catch (const Standard_Failure&) {
        spherical_retry_stop_ = "OCCT exception";
        rollback(); return false;
      } catch (const std::exception&) {
        spherical_retry_stop_ = "exception";
        rollback(); return false;
      }
    } catch (const RollbackFailure&) {
      throw;
    } catch (const Standard_Failure&) {
      return false;
    } catch (const std::exception&) {
      return false;
    }
  }

  std::string boundary_repair_stop_ = "not run";
  std::string spherical_retry_stop_;
  std::string strip_stop_;
  std::string strip_corner_detail_;
  std::map<int,std::string> strip_degenerate_details_;
  std::string strip_repair_stop_ = "strip repair not run";
  std::map<int, std::string> strip_face_rejections_;
  std::size_t strip_comparisons_ = 0;
  int strip_continuous_checks_=0;
  std::size_t pole_certificate_work_ = 0;
  bool native_export_recovery_=false;
  std::size_t export_boundary_work_=0;
  int export_curve_diagnostics_=0;
  int export_boundary_attempts_=0;
  std::string export_boundary_stop_;
  std::map<int,std::string> export_boundary_rejections_;
  TopTools_IndexedMapOfShape strip_original_faces_;
};

static std::string spherical_boundary_failure_detail(const IMeshData::IFaceHandle& face) {
  try {
    if (face->GetSurface()->GetType() != GeomAbs_Sphere) return "";
    std::ostringstream detail, samples, summary;
    detail.precision(10);
    samples.precision(9);
    summary.precision(10);
    struct Segment {
      gp_Pnt2d a, b;
      int edge, index;
      IMeshData::IEdgePtr native_edge;
      double first_parameter, last_parameter;
    };
    std::vector<Segment> segments;
    int edge_count = 0, sample_count = 0;
    bool complete = true;
    const auto finite_uv = [](const gp_Pnt2d& uv) {
      return std::isfinite(uv.X()) && std::isfinite(uv.Y());
    };
    for (int wi = 0; wi < face->WiresNb() && complete; ++wi) {
      const auto& wire = face->GetWire(wi);
      for (int ei = 0; ei < wire->EdgesNb() && complete; ++ei) {
        if (++edge_count > 6) { complete = false; break; }
        const auto& edge = wire->GetEdge(ei);
        const auto orientation = wire->GetEdgeOrientation(ei);
        const auto& pc = edge->GetPCurve(face.get(), orientation);
        const auto& curve = edge->GetCurve();
        if (pc.IsNull() || pc->ParametersNb() != curve->ParametersNb()) {
          complete = false; break;
        }
        BRepAdaptor_Curve on_face(TopoDS::Edge(edge->GetEdge().Oriented(orientation)), face->GetFace());
        const auto& source = on_face.CurveOnSurface().GetCurve();
        double maximum_mesh_error = 0.0, maximum_source_error = 0.0;
        samples << "; sphere wire/edge " << wi << '/' << ei << " boundary samples";
        for (int i = 0; i < pc->ParametersNb(); ++i) {
          if (++sample_count > 24) { complete = false; break; }
          const auto& uv = pc->GetPoint(i);
          const double parameter = pc->GetParameter(i);
          if (!finite_uv(uv) || !std::isfinite(parameter))
            return ", sphere sample diagnostic found nonfinite UV/parameter";
          samples << " [" << i << " t " << parameter << " UV " << uv.X() << ',' << uv.Y();
          const auto mesh_point = face->GetSurface()->Value(uv.X(), uv.Y());
          const double mesh_error = mesh_point.Distance(curve->GetPoint(i));
          if (!std::isfinite(mesh_error)) return ", sphere sample diagnostic found nonfinite distance";
          maximum_mesh_error = std::max(maximum_mesh_error, mesh_error);
          samples << " error mm " << mesh_error;
          if (edge->GetSameParam() && edge->GetSameRange() && !source.IsNull()) {
            const auto source_uv = source->Value(parameter);
            if (!finite_uv(source_uv)) return ", sphere sample diagnostic found nonfinite source UV";
            const double source_error = face->GetSurface()->Value(source_uv.X(), source_uv.Y()).Distance(curve->GetPoint(i));
            if (!std::isfinite(source_error)) return ", sphere sample diagnostic found nonfinite source distance";
            maximum_source_error = std::max(maximum_source_error, source_error);
            samples << " sourceUV " << source_uv.X() << ',' << source_uv.Y()
                   << " source error mm " << source_error;
          }
          samples << ']';
          if (i > 0) segments.push_back({pc->GetPoint(i - 1), uv, edge_count, i - 1,
              edge, pc->GetParameter(i - 1), parameter});
        }
        summary << ", sphere edge " << ei << " max mesh/source error mm "
                << maximum_mesh_error << '/' << maximum_source_error;
      }
    }
    int crossings = 0;
    for (std::size_t a = 0; a < segments.size(); ++a) {
      for (std::size_t b = a + 1; b < segments.size(); ++b) {
        const auto& first = segments[a]; const auto& second = segments[b];
        if (first.edge == second.edge && std::abs(first.index - second.index) <= 1) continue;
        gp_Pnt2d cross;
        if (BRepMesh_GeomTool::IntSegSeg(first.a.Coord(), first.b.Coord(),
                second.a.Coord(), second.b.Coord(), false, false, cross) != BRepMesh_GeomTool::Cross) continue;
        if (!finite_uv(cross)) return ", sphere sample diagnostic found nonfinite crossing";
        ++crossings;
        if (crossings <= 2) {
          detail << ", unfiltered cross edges/segments " << first.edge - 1 << '/' << first.index
                 << ':' << second.edge - 1 << '/' << second.index
                 << " UV " << cross.X() << ',' << cross.Y();
          if (!first.native_edge->GetSameParam() || !first.native_edge->GetSameRange() ||
              !second.native_edge->GetSameParam() || !second.native_edge->GetSameRange()) {
            detail << " native parameter correspondence unavailable";
            continue;
          }
          const auto av = first.b.Coord() - first.a.Coord();
          const auto bv = second.b.Coord() - second.a.Coord();
          const double aa = av.SquareModulus(), bb = bv.SquareModulus();
          if (!std::isfinite(aa) || !std::isfinite(bb) || aa <= 0.0 || bb <= 0.0) continue;
          const double at = first.first_parameter + (first.last_parameter - first.first_parameter) *
              (cross.Coord() - first.a.Coord()).Dot(av) / aa;
          const double bt = second.first_parameter + (second.last_parameter - second.first_parameter) *
              (cross.Coord() - second.a.Coord()).Dot(bv) / bb;
          if (!std::isfinite(at) || !std::isfinite(bt)) continue;
          BRepAdaptor_Curve ac(first.native_edge->GetEdge()), bc(second.native_edge->GetEdge());
          const auto ap = ac.Value(at), bp = bc.Value(bt);
          const auto cross_point = face->GetSurface()->Value(cross.X(), cross.Y());
          const double separation = ap.Distance(bp);
          if (!std::isfinite(separation) || !std::isfinite(cross_point.X()) ||
              !std::isfinite(cross_point.Y()) || !std::isfinite(cross_point.Z())) continue;
          detail << " native parameters " << at << '/' << bt
                 << " native separation mm " << separation;
          if (ac.GetType() == GeomAbs_Circle && bc.GetType() == GeomAbs_Circle &&
              std::isfinite(ac.Circle().Radius()) && std::isfinite(bc.Circle().Radius()) &&
              std::isfinite(ac.Circle().Location().Distance(bc.Circle().Location())))
            detail << " circle radii/center distance mm " << ac.Circle().Radius() << '/'
                   << bc.Circle().Radius() << '/' << ac.Circle().Location().Distance(bc.Circle().Location());
          TopoDS_Vertex a0, a1, b0, b1;
          TopExp::Vertices(first.native_edge->GetEdge(), a0, a1);
          TopExp::Vertices(second.native_edge->GetEdge(), b0, b1);
          for (const auto& vertex : {a0, a1}) {
            if (!vertex.IsNull() && ((!b0.IsNull() && vertex.IsSame(b0)) ||
                                    (!b1.IsNull() && vertex.IsSame(b1))) &&
                std::isfinite(cross_point.Distance(BRep_Tool::Pnt(vertex))) &&
                std::isfinite(BRep_Tool::Tolerance(vertex)))
              detail << " shared vertex distance/tolerance mm "
                     << cross_point.Distance(BRep_Tool::Pnt(vertex)) << '/'
                     << BRep_Tool::Tolerance(vertex);
          }
        }
      }
    }
    if (!complete) detail << " (24-point diagnostic scope limited)";
    return ", unfiltered sphere crossings " + std::to_string(crossings) +
        detail.str().substr(0, 1500) + summary.str().substr(0, 600) + samples.str().substr(0, 1500);
  } catch (const Standard_Failure&) {
    return ", sphere sample diagnostic unavailable (OCCT exception)";
  } catch (const std::exception&) {
    return ", sphere sample diagnostic unavailable (exception)";
  }
}

// Optional diagnostics of a failed face's actual meshing domain. This does not
// change its shared samples, exact geometry or failure status.
static std::string face_mesh_failure_detail(
    const IMeshData::IFaceHandle& face, const IMeshTools_Parameters& parameters) {
  try {
    std::ostringstream detail;
    detail.precision(12);
    TCollection_AsciiString algorithm = OSD_Environment("CSF_MeshAlgo").Value();
    algorithm.LowerCase();
    detail << ", default algorithm "
           << ((algorithm == "delabella" || algorithm == "1") ? "Delabella" : "Watson")
           << ", face orientation " << face->GetFace().Orientation()
           << " tolerance/deflection mm " << BRep_Tool::Tolerance(face->GetFace())
           << '/' << face->GetDeflection()
           << ", requested deflection/min size mm " << parameters.Deflection
           << '/' << parameters.MinSize << " angle " << parameters.Angle
           << " adjust min size " << parameters.AdjustMinSize;
    const auto& surface = face->GetSurface();
    if (surface->GetType() == GeomAbs_Sphere)
      detail << ", sphere radius mm " << surface->Sphere().Radius();
    double u0, u1, v0, v1;
    BRepTools::UVBounds(face->GetFace(), u0, u1, v0, v1);
    if (!std::isfinite(u0) || !std::isfinite(u1) ||
        !std::isfinite(v0) || !std::isfinite(v1))
      return ", face mesh diagnostic found nonfinite UV bounds";
    detail << ", source UV bounds " << u0 << ':' << u1 << '/' << v0 << ':' << v1;
    BRepMesh_SphereRangeSplitter splitter;
    splitter.Reset(face, parameters);
    std::vector<gp_Pnt> points;
    gp_Pnt2d uv_min(std::numeric_limits<double>::max(), std::numeric_limits<double>::max());
    gp_Pnt2d uv_max(-std::numeric_limits<double>::max(), -std::numeric_limits<double>::max());
    gp_Pnt xyz_min(std::numeric_limits<double>::max(), std::numeric_limits<double>::max(),
                   std::numeric_limits<double>::max());
    gp_Pnt xyz_max(-std::numeric_limits<double>::max(), -std::numeric_limits<double>::max(),
                   -std::numeric_limits<double>::max());
    int edge_count = 0;
    bool complete = true;
    for (int wi = 0; wi < face->WiresNb(); ++wi) {
      const auto& wire = face->GetWire(wi);
      std::vector<gp_Pnt2d> polygon;
      for (int ei = 0; ei < wire->EdgesNb(); ++ei) {
        if (++edge_count > 6) { complete = false; break; }
        const auto& edge = wire->GetEdge(ei);
        const auto orientation = wire->GetEdgeOrientation(ei);
        const auto& pc = edge->GetPCurve(face.get(), orientation);
        if (pc.IsNull()) { complete = false; continue; }
        const auto& curve = edge->GetCurve();
        if (curve->ParametersNb() != pc->ParametersNb()) { complete = false; continue; }
        const int count = pc->ParametersNb();
        detail << "; edge " << ei << " kind " << BRepAdaptor_Curve(edge->GetEdge()).GetType()
               << " orientation " << orientation << " tolerance mm "
               << BRep_Tool::Tolerance(edge->GetEdge()) << " degenerate "
               << BRep_Tool::Degenerated(edge->GetEdge());
        for (int i = 0; i < count; ++i) {
          if (points.size() >= 2048) { complete = false; break; }
          const auto& uv = pc->GetPoint(i);
          const auto& point = curve->GetPoint(i);
          if (!std::isfinite(uv.X()) || !std::isfinite(uv.Y()) ||
              !std::isfinite(point.X()) || !std::isfinite(point.Y()) ||
              !std::isfinite(point.Z()))
            return ", face mesh diagnostic found nonfinite boundary samples";
          splitter.AddPoint(uv);
          uv_min.SetX(std::min(uv_min.X(), uv.X()));
          uv_min.SetY(std::min(uv_min.Y(), uv.Y()));
          uv_max.SetX(std::max(uv_max.X(), uv.X()));
          uv_max.SetY(std::max(uv_max.Y(), uv.Y()));
          for (int axis = 1; axis <= 3; ++axis) {
            xyz_min.SetCoord(axis, std::min(xyz_min.Coord(axis), point.Coord(axis)));
            xyz_max.SetCoord(axis, std::max(xyz_max.Coord(axis), point.Coord(axis)));
          }
          points.push_back(point);
          if (i == 0 || i == count / 2 || i == count - 1)
            detail << " UV[" << i << "] " << uv.X() << ',' << uv.Y();
        }
        // The oriented wire polygon excludes each edge's duplicate ending node,
        // matching NodeInsertionMeshAlgo's collection of boundary points.
        for (int i = 0; i < count - 1 && polygon.size() < 2048; ++i)
          polygon.push_back(pc->GetPoint(orientation == TopAbs_REVERSED ? count - 1 - i : i));
        if (!complete) break;
      }
      if (polygon.size() >= 3) {
        double twice_area = 0.0;
        const auto origin = polygon.front().Coord();
        for (std::size_t i = 0; i < polygon.size(); ++i)
          twice_area += (polygon[i].Coord() - origin).Crossed(
              polygon[(i + 1) % polygon.size()].Coord() - origin);
        if (std::isfinite(twice_area))
          detail << ", wire " << wi << " signed UV area " << 0.5 * twice_area;
      }
      if (!complete) break;
    }
    if (!points.empty()) {
      double minimum_gap = std::numeric_limits<double>::max();
      int coincident_pairs = 0;
      for (std::size_t i = 0; i < points.size(); ++i) {
        for (std::size_t j = i + 1; j < points.size(); ++j) {
          const double gap = points[i].Distance(points[j]);
          if (!std::isfinite(gap)) return ", face mesh diagnostic found nonfinite distance";
          if (gap == 0.0) ++coincident_pairs;
          else minimum_gap = std::min(minimum_gap, gap);
        }
      }
      detail << ", sampled UV bounds " << uv_min.X() << ':' << uv_max.X()
             << '/' << uv_min.Y() << ':' << uv_max.Y()
             << ", boundary XYZ span mm " << xyz_max.X() - xyz_min.X() << ','
             << xyz_max.Y() - xyz_min.Y() << ',' << xyz_max.Z() - xyz_min.Z();
      if (minimum_gap != std::numeric_limits<double>::max())
        detail << " min positive gap mm " << minimum_gap;
      detail << " coincident pairs " << coincident_pairs;
      if (complete && surface->GetType() == GeomAbs_Sphere) {
        splitter.AdjustRange();
        detail << ", sphere splitter valid " << splitter.IsValid()
               << " UV tolerance " << splitter.GetToleranceUV().first << '/'
               << splitter.GetToleranceUV().second << " delta "
               << splitter.GetDelta().first << '/' << splitter.GetDelta().second;
      }
    }
    if (!complete) detail << ", face mesh diagnostic scope limited";
    return detail.str().substr(0, 2200);
  } catch (const Standard_Failure&) {
    return ", face mesh diagnostic unavailable (OCCT exception)";
  } catch (const std::exception&) {
    return ", face mesh diagnostic unavailable (exception)";
  }
}

static std::string boundary_failure_detail(
    const IMeshData::IFaceHandle& face, const IMeshTools_Parameters& parameters) {
  try {
    BRepMesh_FaceChecker checker(face, parameters);
    if (checker.Perform()) return ", no reported boundary intersections";
    const auto& intersections = checker.GetIntersectingEdges();
    if (intersections.IsNull()) return ", no crossing edge map";
    struct Edge {
      IMeshData::IEdgePtr edge;
      IMeshData::IPCurveHandle pcurve;
      int wire;
      int index;
    };
    std::vector<Edge> edges;
    for (int wi = 0; wi < face->WiresNb() && edges.size() < 6; ++wi) {
      const auto& wire = face->GetWire(wi);
      for (int ei = 0; ei < wire->EdgesNb() && edges.size() < 6; ++ei) {
        auto edge = wire->GetEdge(ei);
        if (!intersections->Contains(edge)) continue;
        const auto& pcurve = edge->GetPCurve(face.get(), wire->GetEdgeOrientation(ei));
        if (!pcurve.IsNull() && pcurve->ParametersNb() >= 2)
          edges.push_back({edge, pcurve, wi, ei});
      }
    }
    std::size_t comparisons = 0;
    const auto finite_uv = [](const gp_Pnt2d& point) {
      return std::isfinite(point.X()) && std::isfinite(point.Y());
    };
    for (std::size_t a = 0; a < edges.size(); ++a) {
      for (std::size_t b = a + 1; b < edges.size(); ++b) {
        const auto& ae = edges[a]; const auto& be = edges[b];
        for (int ai = 1; ai < ae.pcurve->ParametersNb(); ++ai) {
          const auto& p = ae.pcurve->GetPoint(ai - 1);
          const auto& q = ae.pcurve->GetPoint(ai);
          for (int bi = 1; bi < be.pcurve->ParametersNb(); ++bi) {
            if (++comparisons > 16000000) return ", crossing diagnostic search limit";
            const auto& r = be.pcurve->GetPoint(bi - 1);
            const auto& s = be.pcurve->GetPoint(bi);
            if (!finite_uv(p) || !finite_uv(q) || !finite_uv(r) || !finite_uv(s))
              return ", boundary diagnostic found nonfinite UV samples";
            if (std::max(p.X(), q.X()) < std::min(r.X(), s.X()) ||
                std::max(r.X(), s.X()) < std::min(p.X(), q.X()) ||
                std::max(p.Y(), q.Y()) < std::min(r.Y(), s.Y()) ||
                std::max(r.Y(), s.Y()) < std::min(p.Y(), q.Y())) continue;
            gp_Pnt2d intersection;
            if (BRepMesh_GeomTool::IntSegSeg(p.Coord(), q.Coord(), r.Coord(), s.Coord(),
                    false, false, intersection) != BRepMesh_GeomTool::Cross) continue;
            if (!finite_uv(intersection))
              return ", boundary diagnostic found nonfinite intersection";
            const gp_XY av = q.Coord() - p.Coord(), bv = s.Coord() - r.Coord();
            const double aa = av.SquareModulus(), bb = bv.SquareModulus();
            const double dot = av.Dot(bv);
            if (!std::isfinite(aa) || !std::isfinite(bb) || !std::isfinite(dot) ||
                aa <= 0.0 || bb <= 0.0) continue;
            const double cosine = (dot / std::sqrt(aa)) / std::sqrt(bb);
            if (!std::isfinite(cosine)) continue;
            const double angle = std::acos(std::clamp(cosine, -1.0, 1.0));
            if (angle < kPi / 36.0) continue;
            const double af = (intersection.Coord() - p.Coord()).Dot(av) / av.SquareModulus();
            const double bf = (intersection.Coord() - r.Coord()).Dot(bv) / bv.SquareModulus();
            const double at = ae.pcurve->GetParameter(ai - 1) +
                af * (ae.pcurve->GetParameter(ai) - ae.pcurve->GetParameter(ai - 1));
            const double bt = be.pcurve->GetParameter(bi - 1) +
                bf * (be.pcurve->GetParameter(bi) - be.pcurve->GetParameter(bi - 1));
            if (!std::isfinite(at) || !std::isfinite(bt))
              return ", boundary diagnostic found nonfinite curve parameters";
            std::ostringstream detail;
            detail.precision(12);
            detail << ", crossing wires/edges " << ae.wire << '/' << ae.index
                   << ':' << be.wire << '/' << be.index << " segments "
                   << ai - 1 << ':' << bi - 1 << " UV " << intersection.X()
                   << ',' << intersection.Y() << " angle deg " << angle * 180.0 / kPi
                   << " parameters " << at << ':' << bt
                   << " intervals " << ae.pcurve->GetParameter(ai - 1) << ':'
                   << ae.pcurve->GetParameter(ai) << '/'
                   << be.pcurve->GetParameter(bi - 1) << ':' << be.pcurve->GetParameter(bi)
                   << " segment UV " << p.X() << ',' << p.Y() << ':' << q.X() << ',' << q.Y()
                   << '/' << r.X() << ',' << r.Y() << ':' << s.X() << ',' << s.Y()
                   << " face deflection " << face->GetDeflection();
            const gp_Pnt cross_point = face->GetSurface()->Value(intersection.X(), intersection.Y());
            TopoDS_Vertex vertex;
            const bool has_vertex = TopExp::CommonVertex(ae.edge->GetEdge(), be.edge->GetEdge(), vertex);
            if (has_vertex) {
              detail << " vertex distance/tolerance mm "
                     << cross_point.Distance(BRep_Tool::Pnt(vertex)) << '/'
                     << BRep_Tool::Tolerance(vertex);
            }
            for (const auto& item : {std::make_pair(ae, at), std::make_pair(be, bt)}) {
              const auto& edge = item.first;
              const auto& pc = edge.pcurve;
              detail << "; edge " << edge.index << " orientation " << pc->GetOrientation()
                     << " same param/range " << edge.edge->GetSameParam() << '/'
                     << edge.edge->GetSameRange() << " tolerance mm "
                     << BRep_Tool::Tolerance(edge.edge->GetEdge());
              if (!edge.edge->GetSameParam() || !edge.edge->GetSameRange()) continue;
              BRepAdaptor_Curve native(edge.edge->GetEdge());
              BRepAdaptor_Curve on_face(
                  TopoDS::Edge(edge.edge->GetEdge().Oriented(pc->GetOrientation())), face->GetFace());
              const auto& exact_pc = on_face.CurveOnSurface().GetCurve();
              detail << " endpoints";
              for (int index : {0, pc->ParametersNb() - 1}) {
                const auto& uv = pc->GetPoint(index);
                const double t = pc->GetParameter(index);
                if (!finite_uv(uv) || !std::isfinite(t))
                  return ", boundary diagnostic found nonfinite endpoint";
                const auto native_uv = exact_pc->Value(t);
                if (!finite_uv(native_uv))
                  return ", boundary diagnostic found nonfinite source endpoint";
                const auto native_point = native.Value(t);
                detail << " [" << t << " UV " << uv.X() << ',' << uv.Y()
                       << " sourceUV " << native_uv.X() << ',' << native_uv.Y()
                       << " shift/error mm "
                       << face->GetSurface()->Value(uv.X(), uv.Y()).Distance(
                              face->GetSurface()->Value(native_uv.X(), native_uv.Y())) << '/'
                       << face->GetSurface()->Value(uv.X(), uv.Y()).Distance(native_point)
                       << " source error mm "
                       << face->GetSurface()->Value(native_uv.X(), native_uv.Y()).Distance(native_point) << ']';
              }
              const auto native_uv = exact_pc->Value(item.second);
              if (!finite_uv(native_uv))
                return ", boundary diagnostic found nonfinite source intersection";
              const auto native_point = native.Value(item.second);
              detail << " crossing chord/source error mm " << cross_point.Distance(native_point)
                     << '/' << face->GetSurface()->Value(native_uv.X(), native_uv.Y()).Distance(native_point);
              if (has_vertex)
                detail << " native vertex distance mm " << native_point.Distance(BRep_Tool::Pnt(vertex));
            }
            return detail.str().substr(0, 2400);
          }
        }
      }
    }
    return ", no different-edge crossing found in diagnostic scope";
  } catch (const Standard_Failure&) {
    return ", boundary diagnostic unavailable (OCCT failure)";
  } catch (const std::exception&) {
    return ", boundary diagnostic unavailable (native exception)";
  }
}

// Export indexing follows native topology, not coordinate proximity. Two
// contacting shells can have identical coordinates and still distinct vertices.
class NativeExportIndex {
 public:
  using Key = std::array<int,4>; // shell, kind (face/vertex/edge), shape, sample
  struct Node { Key key; double tolerance; };
  struct Sample { std::uint32_t index; gp_Pnt point; };
  struct Facet { int face,triangle; std::array<int,3> nodes; };
  NativeExportIndex(const TopoDS_Shape& shape, const TopTools_IndexedMapOfShape& edges, double deflection,
                    const std::string& recovery,const std::map<int,std::string>& rejections)
      : shape_(shape), edges_(edges), recovery_(recovery), rejections_(rejections), deflection_(deflection) {
    TopExp::MapShapes(shape,TopAbs_SHELL,shells_);
    TopExp::MapShapes(shape,TopAbs_VERTEX,vertices_);
    TopExp::MapShapesAndUniqueAncestors(shape,TopAbs_FACE,TopAbs_SHELL,face_shells_,false);
    closed_ = shells_.Extent()>0;
    for (int si=1;si<=shells_.Extent();++si) closed_ &= BRep_Tool::IsClosed(shells_.FindKey(si));
  }
  std::vector<Node> face_nodes(const TopoDS_Face& face, int face_index,
                              const Handle(Poly_Triangulation)& mesh, const TopLoc_Location& location) {
    int owner = -face_index-1; // A standalone surface has no shared shell owner.
    if (face_shells_.Contains(face)) {
      const auto& shells = face_shells_.FindFromKey(face);
      if (shells.Extent()>1) fail(face_index,"face has multiple native shell owners");
      if (shells.Extent()==1) owner = shells_.FindIndex(shells.First());
    }
    if (owner<0) closed_=false;
    face_meshes_[face_index-1]=mesh;
    if (mesh->NbNodes()>1000000 || (node_work_+=mesh->NbNodes())>8000000)
      fail(face_index,"native node budget exceeded");
    std::vector<Node> nodes(mesh->NbNodes()+1);
    for (int ni=1;ni<=mesh->NbNodes();++ni) nodes[ni]={{owner,0,face_index,ni},0.0};
    const auto assign = [&](int id,const Key& key,double tolerance) {
      if (id<1 || id>mesh->NbNodes() || !std::isfinite(tolerance) || tolerance<0.0)
        fail(face_index,"invalid boundary node/tolerance");
      if (nodes[id].key[1]!=0 && nodes[id].key!=key) {
        std::ostringstream detail;detail.precision(12);
        const auto point=mesh->Node(id).Transformed(location.Transformation());
        detail << "node " << id << " has conflicting native vertex/edge identities; world " <<
            point.X() << '/' << point.Y() << '/' << point.Z();
        if (mesh->HasUVNodes()) detail << " UV " << mesh->UVNode(id).X() << '/' << mesh->UVNode(id).Y();
        const auto describe=[&](const char* label,const Key& identity,double tol) {
          detail << "; " << label << " shell/kind/shape/slot " << identity[0] << '/' << identity[1] << '/' <<
              identity[2]-1 << '/' << identity[3] << " tolerance " << tol;
          if (identity[1]==1 && identity[2]>=1 && identity[2]<=vertices_.Extent()) {
            const auto native=BRep_Tool::Pnt(TopoDS::Vertex(vertices_.FindKey(identity[2])));
            detail << " native vertex XYZ " << native.X() << '/' << native.Y() << '/' << native.Z() <<
                " node gap " << native.Distance(point);
          } else if (identity[1]==2 && identity[2]>=1 && identity[2]<=edges_.Extent()) {
            const auto parameters=edge_parameters_.find({identity[0],identity[2]});
            if (parameters!=edge_parameters_.end() && identity[3]>=1 &&
                static_cast<std::size_t>(identity[3])<=parameters->second.size()) {
              const double parameter=parameters->second[identity[3]-1];
              const auto native=BRepAdaptor_Curve(TopoDS::Edge(edges_.FindKey(identity[2]))).Value(parameter);
              detail << " native parameter " << parameter << " curve XYZ " << native.X() << '/' << native.Y() << '/' <<
                  native.Z() << " node gap " << native.Distance(point);
            }
          }
        };
        // Optional diagnostics cannot mask the original strict identity error.
        try {
          describe("existing",nodes[id].key,nodes[id].tolerance);describe("incoming",key,tolerance);
          for (const auto& identities : {std::make_pair(nodes[id].key,key),std::make_pair(key,nodes[id].key)}) {
            const auto& vertex_key=identities.first;const auto& edge_key=identities.second;
            if (vertex_key[1]!=1 || edge_key[1]!=2 || vertex_key[2]<1 || vertex_key[2]>vertices_.Extent() ||
                edge_key[2]<1 || edge_key[2]>edges_.Extent()) continue;
            const auto vertex=TopoDS::Vertex(vertices_.FindKey(vertex_key[2]));
            const auto edge=TopoDS::Edge(edges_.FindKey(edge_key[2]));int occurrences=0;
            for (TopExp_Explorer explorer(edge,TopAbs_VERTEX);explorer.More();explorer.Next())
              if (explorer.Current().IsSame(vertex)) ++occurrences;
            detail << "; native vertex-in-edge occurrences " << occurrences;
            const auto parameters=edge_parameters_.find({edge_key[0],edge_key[2]});
            if (occurrences && parameters!=edge_parameters_.end() && edge_key[3]>=1 &&
                static_cast<std::size_t>(edge_key[3])<=parameters->second.size()) {
              const double native=BRep_Tool::Parameter(vertex,edge),sample=parameters->second[edge_key[3]-1];
              const double roundoff=128.0*std::numeric_limits<double>::epsilon()*std::max({1.0,std::abs(native),std::abs(sample)});
              detail << " vertex/sample parameters " << native << '/' << sample << " difference/roundoff " << std::abs(native-sample) << '/' << roundoff;
            }
          }
        } catch (const Standard_Failure&) { detail << "; diagnostic OCCT exception"; }
          catch (const std::exception&) { detail << "; diagnostic native exception"; }
        std::fprintf(stderr,"Native export identity face %d: %s\n",face_index,detail.str().substr(0,2400).c_str());
        fail(face_index,detail.str().substr(0,2400));
      }
      nodes[id]={key,tolerance};
    };
    // Keep oriented occurrences: a seam has two polygons on the same face.
    for (TopExp_Explorer explorer(face,TopAbs_EDGE);explorer.More();explorer.Next()) {
      const auto edge=TopoDS::Edge(explorer.Current());
      const int ei=edges_.FindIndex(edge);
      const auto polygon=BRep_Tool::PolygonOnTriangulation(edge,mesh,location);
      if (ei<=0 || polygon.IsNull() || polygon->NbNodes()<2 || polygon->NbNodes()>65536)
        fail(face_index,"edge "+std::to_string(ei-1)+" has no bounded native boundary polygon");
      TopoDS_Vertex first,last; TopExp::Vertices(edge,first,last,false);
      if (first.IsNull() || last.IsNull()) fail(face_index,"edge "+std::to_string(ei-1)+" lacks native endpoints");
      if (BRep_Tool::Degenerated(edge)) {
        if (polygon->NbNodes()!=2 || !first.IsSame(last))
          fail(face_index,"native degenerate edge "+std::to_string(ei-1)+" has ambiguous pole ownership");
        const int vi=vertices_.FindIndex(first);
        if (vi<=0) fail(face_index,"native pole vertex is absent from shape");
        assign(polygon->Node(1),{owner,1,vi,0},BRep_Tool::Tolerance(first));
        assign(polygon->Node(2),{owner,1,vi,0},BRep_Tool::Tolerance(first));
        continue;
      }
      if (!polygon->HasParameters() || !BRep_Tool::SameParameter(edge) || !BRep_Tool::SameRange(edge))
        fail(face_index,"edge "+std::to_string(ei-1)+" lacks common native sample parameters");
      double range_first,range_last; BRep_Tool::Range(edge,range_first,range_last);
      const double parameter_roundoff=128.0*std::numeric_limits<double>::epsilon()*
          std::max({1.0,std::abs(range_first),std::abs(range_last)});
      if (!std::isfinite(range_first) || !std::isfinite(range_last) || range_last<=range_first ||
          !std::isfinite(polygon->Parameter(1)) || !std::isfinite(polygon->Parameter(polygon->NbNodes())) ||
          std::abs(polygon->Parameter(1)-range_first)>parameter_roundoff ||
          std::abs(polygon->Parameter(polygon->NbNodes())-range_last)>parameter_roundoff)
        fail(face_index,"edge "+std::to_string(ei-1)+" has ambiguous endpoint parameter ordering");
      const auto edge_key=std::make_pair(owner,ei);
      auto found=edge_parameters_.find(edge_key);
      if (found==edge_parameters_.end()) {
        if ((parameter_work_+=polygon->NbNodes())>2000000) fail(face_index,"native edge parameter budget exceeded");
        std::vector<double> parameters;
        for (int i=1;i<=polygon->NbNodes();++i) {
          const double parameter=polygon->Parameter(i);
          if (!std::isfinite(parameter) || (i>1 && parameter<=parameters.back()))
            fail(face_index,"edge "+std::to_string(ei-1)+" sample parameters are not strictly ordered");
          parameters.push_back(parameter);
        }
        found=edge_parameters_.emplace(edge_key,std::move(parameters)).first;
      }
      if (found->second.size()!=static_cast<std::size_t>(polygon->NbNodes()))
        fail(face_index,"edge "+std::to_string(ei-1)+" sample count differs across owners: "+
            std::to_string(found->second.size())+"/"+std::to_string(polygon->NbNodes()));
      for (int i=1;i<=polygon->NbNodes();++i) {
        if (!std::isfinite(polygon->Parameter(i)) ||
            std::abs(polygon->Parameter(i)-found->second[i-1])>parameter_roundoff)
          fail(face_index,"edge "+std::to_string(ei-1)+" sample "+std::to_string(i)+" native parameters differ");
        if (i==1 || i==polygon->NbNodes()) {
          const auto vertex=i==1 ? first : last;
          const int vi=vertices_.FindIndex(vertex);
          if (vi<=0) fail(face_index,"native boundary vertex is absent from shape");
          assign(polygon->Node(i),{owner,1,vi,0},BRep_Tool::Tolerance(vertex));
        } else {
          assign(polygon->Node(i),{owner,2,ei,i},BRep_Tool::Tolerance(edge));
        }
      }
    }
    return nodes;
  }
  Sample sample(const Node& node,const gp_Pnt& point,const gp_Dir& normal,FfiMesh& output,int face_index) {
    const auto found=samples_.find(node.key);
    if (found!=samples_.end()) {
      const double gap=found->second.point.Distance(point);
      if (!std::isfinite(gap) || gap>std::min(node.tolerance,deflection_/16.0)) {
        std::ostringstream detail; detail.precision(12);
        detail << "native shared sample kind/shape/slot " << node.key[1] << '/' << node.key[2]-1 << '/' << node.key[3] <<
            " gap " << gap << " tolerance/precision " << node.tolerance << '/' << deflection_/16.0;
        fail(face_index,detail.str());
      }
      return found->second;
    }
    const std::uint32_t index=static_cast<std::uint32_t>(output.positions.size()/3);
    output.positions.push_back(static_cast<float>(point.X()));
    output.positions.push_back(static_cast<float>(point.Y()));
    output.positions.push_back(static_cast<float>(point.Z()));
    append_point(output.export_positions,point);
    append_vec(output.normals,gp_Vec(normal));
    vertex_keys_.push_back(node.key);
    vertex_tolerances_.push_back(node.tolerance);
    const Sample value{index,point}; samples_.emplace(node.key,value); return value;
  }
  void facet(int face,int triangle,const int* nodes,bool skipped,double cross2=0.0) {
    const Facet value{face-1,triangle,{nodes[0],nodes[1],nodes[2]}};
    if (!skipped) facets_.push_back(value);
    else {
      ++skipped_faces_[face-1];
      if (cross2>0.0) ++positive_skipped_faces_[face-1];
      if (skipped_facets_.size()<4) {
        skipped_facets_.push_back(value);
        skipped_cross2_.push_back(cross2);
      }
    }
  }
  void retained_small(int face) { ++retained_small_faces_[face-1]; }
  void validate(const FfiMesh& output) const {
    // Open surface STL remains supported; 3MF independently requires closure.
    if (!closed_) return;
    struct Use { int count=0,balance=0; std::array<std::pair<std::size_t,int>,4> incidents; };
    std::map<std::pair<std::uint32_t,std::uint32_t>,Use> uses;
    for (std::size_t i=0;i<output.indices.size();i+=3) for (int j=0;j<3;++j) {
      const auto a=output.indices[i+j],b=output.indices[i+(j+1)%3];
      auto& use=uses[{std::min(a,b),std::max(a,b)}];
      if (use.count<4) use.incidents[use.count]={i/3,j};
      ++use.count; use.balance+=a<b ? 1 : -1;
    }
    std::size_t invalid=0;
    std::map<int,int> face_groups,edge_groups;
    std::map<std::pair<int,int>,int> invalid_types;
    std::set<int> four_use_edges;
    std::vector<std::pair<std::pair<std::uint32_t,std::uint32_t>,Use>> failures;
    std::map<int,std::pair<std::pair<std::uint32_t,std::uint32_t>,Use>> examples_by_face;
    for (const auto& use : uses) if (use.second.count!=2 || use.second.balance!=0) {
      ++invalid;
      ++invalid_types[{use.second.count,use.second.balance}];
      for (int i=0;i<std::min(4,use.second.count);++i) {
        const int face=facets_.at(use.second.incidents[i].first).face;
        ++face_groups[face]; examples_by_face.emplace(face,use);
      }
      for (const auto id : {use.first.first,use.first.second}) if (vertex_keys_.at(id)[1]==2) {
        ++edge_groups[vertex_keys_[id][2]-1];
        if (use.second.count>2) four_use_edges.insert(vertex_keys_[id][2]);
      }
    }
    if (!invalid) return;
    // Keep the complete small residual set available outside the UI's bounded
    // error text, so unrelated rejected candidates are not mistaken for holes.
    std::ostringstream residual;residual.precision(12);
    residual << "Native export remaining links " << invalid << " native face:incident-use groups";
    int group_count=0;
    for (const auto& group : face_groups) { if (group_count++==32) { residual << " [further groups]";break; } residual << ' ' << group.first << ':' << group.second; }
    std::fprintf(stderr,"%s\n",residual.str().substr(0,3000).c_str());
    int link_count=0;
    for (const auto& use : uses) if (use.second.count!=2 || use.second.balance!=0) {
      if (link_count++==32) break;
      std::ostringstream item;
      item << "Native export residual " << use.first.first << '/' << use.first.second << " uses/balance " << use.second.count << '/' << use.second.balance;
      for (const auto id : {use.first.first,use.first.second}) {
        const auto& key=vertex_keys_.at(id);
        item << " shell/kind/shape/slot " << key[0] << '/' << key[1] << '/' << key[2]-1 << '/' << key[3];
      }
      for (int i=0;i<std::min(4,use.second.count);++i) {
        const auto& facet=facets_.at(use.second.incidents[i].first);
        item << " face/tri " << facet.face << '/' << facet.triangle;
      }
      std::fprintf(stderr,"%s\n",item.str().substr(0,1500).c_str());
    }
    std::ostringstream details; details.precision(12);
    details << "Native export topology is not a closed oriented mesh: " << invalid <<
        " invalid links; " << shells_.Extent() << " native shells; " << recovery_ << ". Invalid uses/balance:count";
    int types=0;
    for (const auto& type : invalid_types) {
      details << ' ' << type.first.first << '/' << type.first.second << ':' << type.second;
      if (++types==8) { if (invalid_types.size()>8) details << " [further types]"; break; }
    }
    const std::size_t catalog_position=details.str().size();
    std::string wire_catalog;
    int multi_examples=0;
    details << ". Multi-use incident face/tri/native-nodes";
    for (const auto& use : uses) if (use.second.count>2) {
      details << " [link " << use.first.first << '/' << use.first.second << " uses " << use.second.count;
      for (int i=0;i<std::min(4,use.second.count);++i) {
        const auto& incident=use.second.incidents[i]; const auto& facet=facets_.at(incident.first);
        details << ' ' << facet.face << '/' << facet.triangle << ':' << facet.nodes[0] << ',' << facet.nodes[1] << ',' << facet.nodes[2];
      }
      details << ']';
      if (++multi_examples==2) break;
    }
    // Source incidence counts oriented OCCURRENCES in the same native shell,
    // including both occurrences of a seam. Unique ancestor counts alone
    // cannot distinguish a seam, omitted mesh owner, or nonmanifold edge.
    try {
      std::vector<std::pair<int,int>> worst_edges;
      for (const auto& edge : edge_groups) worst_edges.emplace_back(edge.second,edge.first);
      std::sort(worst_edges.begin(),worst_edges.end(),[](const auto& a,const auto& b) { return a>b; });
      std::set<int> selected_edges;
      for (std::size_t i=0;i<std::min<std::size_t>(6,worst_edges.size());++i) selected_edges.insert(worst_edges[i].second+1);
      for (int edge : four_use_edges) { if (selected_edges.size()>=6) break; selected_edges.insert(edge); }
      struct NativeOwner { int face,orientation,nodes;std::string stage,physical; };
      struct NativeUse { int count=0,balance=0; std::set<int> faces; std::vector<NativeOwner> owners; };
      std::map<std::pair<int,int>,NativeUse> native_uses;
      TopTools_IndexedMapOfShape native_faces; TopExp::MapShapes(shape_,TopAbs_FACE,native_faces);
      const auto finite_uv=[](const gp_Pnt2d& p) { return std::isfinite(p.X()) && std::isfinite(p.Y()); };
      int chart_examples=0;
      details << ". Multi-use UV centroid/state/signed-area";
      for (const auto& use : uses) if (use.second.count>2) {
        details << " [link " << use.first.first << '/' << use.first.second;
        for (int i=0;i<std::min(4,use.second.count);++i) {
          const auto& facet=facets_.at(use.second.incidents[i].first);
          const auto& mesh=face_meshes_.at(facet.face);
          if (!mesh->HasUVNodes()) continue;
          const auto a=mesh->UVNode(facet.nodes[0]),b=mesh->UVNode(facet.nodes[1]),c=mesh->UVNode(facet.nodes[2]);
          const gp_Pnt2d center((a.Coord()+b.Coord()+c.Coord())/3.0);
          if (!finite_uv(center) || !finite_uv(a) || !finite_uv(b) || !finite_uv(c)) {
            details << ' ' << facet.face << ":nonfinite UV"; continue;
          }
          const auto face=TopoDS::Face(native_faces.FindKey(facet.face+1));
          BRepClass_FaceClassifier classifier(face,center,Precision::PConfusion());
          details << ' ' << facet.face << ':' << center.X() << ',' << center.Y() << '/' <<
              static_cast<int>(classifier.State()) << '/' << .5*(b.Coord()-a.Coord()).Crossed(c.Coord()-a.Coord());
        }
        details << ']';
        if (++chart_examples==2) break;
      }
      std::size_t native_work=0,physical_work=0;
      for (int si=1;si<=shells_.Extent();++si) for (TopExp_Explorer fe(shells_.FindKey(si),TopAbs_FACE);fe.More();fe.Next()) {
        const auto face=TopoDS::Face(fe.Current());
        const int face_index=native_faces.FindIndex(face)-1;
        for (TopExp_Explorer ee(face,TopAbs_EDGE);ee.More();ee.Next()) {
          if (++native_work>200000) throw std::runtime_error("native incidence diagnostic budget");
          const auto edge=TopoDS::Edge(ee.Current()); const int ei=edges_.FindIndex(edge);
          if (!selected_edges.count(ei)) continue;
          auto& use=native_uses[{si,ei}]; ++use.count; use.faces.insert(face_index);
          const auto orientation=edge.Orientation();
          use.balance+=orientation==TopAbs_FORWARD ? 1 : orientation==TopAbs_REVERSED ? -1 : 0;
          if (use.owners.size()<6) {
            TopLoc_Location location;
            const auto mesh=BRep_Tool::Triangulation(face,location);
            const auto polygon=mesh.IsNull() ? Handle(Poly_PolygonOnTriangulation)() :
                BRep_Tool::PolygonOnTriangulation(edge,mesh,location);
            std::ostringstream physical;physical.precision(9);
            const auto rejection=rejections_.find(face_index);
            const std::string owner_stage=rejection==rejections_.end() ? "unrecorded" : rejection->second.substr(0,1300);
            if (!mesh.IsNull() && !polygon.IsNull()) {
              std::map<std::pair<int,int>,std::array<int,2>> raw;
              int zero=0,positive=0;std::string zero_example;
              for (int ti=1;ti<=mesh->NbTriangles();++ti) {
                if (++physical_work>2097152) throw std::runtime_error("physical owner diagnostic budget");
                int ids[3];mesh->Triangle(ti).Get(ids[0],ids[1],ids[2]);
                gp_Pnt p[3];for (int i=0;i<3;++i) {
                  if (ids[i]<1 || ids[i]>mesh->NbNodes()) throw std::runtime_error("physical owner diagnostic node");
                  p[i]=mesh->Node(ids[i]).Transformed(location.Transformation());
                }
                const double area=gp_Vec(p[0],p[1]).Crossed(gp_Vec(p[0],p[2])).SquareMagnitude();
                if (!std::isfinite(area)) throw std::runtime_error("physical owner diagnostic area");
                if (area==0.0) {
                  ++zero;
                  if (zero_example.empty() && mesh->HasUVNodes()) {
                    const auto a=mesh->UVNode(ids[0]),b=mesh->UVNode(ids[1]),c=mesh->UVNode(ids[2]);
                    std::ostringstream cell;cell.precision(9);
                    cell << " zero tri/nodes/UVarea " << ti << ':' << ids[0] << '/' << ids[1] << '/' << ids[2] << ':' <<
                        .5*(b.Coord()-a.Coord()).Crossed(c.Coord()-a.Coord());zero_example=cell.str();
                  }
                } else ++positive;
                for (int i=0;i<3;++i) {
                  auto& counts=raw[{std::min(ids[i],ids[(i+1)%3]),std::max(ids[i],ids[(i+1)%3])}];
                  ++counts[0];if (area>0.0) ++counts[1];
                }
              }
              physical << " facets positive/zero " << positive << '/' << zero << " deficient POT slot:nodes:raw/positive";
              int shown=0;
              for (int i=1;i<polygon->NbNodes();++i) {
                const int a=polygon->Node(i),b=polygon->Node(i+1);
                const auto found=raw.find({std::min(a,b),std::max(a,b)});
                const auto counts=found==raw.end() ? std::array<int,2>{0,0} : found->second;
                if (counts[1]==1) continue;
                physical << ' ' << i-1 << ':' << a << '/' << b << ':' << counts[0] << '/' << counts[1];
                if (++shown==3) break;
              }
              physical << zero_example;
            }
            use.owners.push_back({face_index,static_cast<int>(orientation),polygon.IsNull() ? 0 : polygon->NbNodes(),owner_stage,physical.str()});
          }
        }
      }
      // Put distinct spherical owner constraints before repeated edge stories,
      // so the bounded UI error identifies extra edges blocking strip recovery.
      std::set<int> catalog_faces;
      std::ostringstream catalog;catalog.precision(8);
      for (const auto& entry : native_uses) for (const auto& owner : entry.second.owners) {
        if (catalog_faces.count(owner.face) || catalog_faces.size()>=3) continue;
        try {
        const auto face=TopoDS::Face(native_faces.FindKey(owner.face+1));
        if (BRepAdaptor_Surface(face).GetType()!=GeomAbs_Sphere) continue;
        catalog_faces.insert(owner.face);
        const auto mesh=face_meshes_.at(owner.face);TopLoc_Location location;
        BRep_Tool::Triangulation(face,location);
        double u0,u1,v0,v1;BRepTools::UVBounds(face,u0,u1,v0,v1);
        catalog << " [face " << owner.face << " sphere R " << BRepAdaptor_Surface(face).Sphere().Radius() <<
            " UV " << u0 << '/' << u1 << ':' << v0 << '/' << v1;
        int wi=0,edge_count=0;
        for (TopExp_Explorer wires(face,TopAbs_WIRE);wires.More();wires.Next(),++wi) {
          catalog << " wire " << wi;
          for (BRepTools_WireExplorer occurrence(TopoDS::Wire(wires.Current()),face);occurrence.More();occurrence.Next()) {
            if (++edge_count>6) { catalog << " [edge cap]";break; }
            const auto edge=occurrence.Current();TopoDS_Vertex first,last;TopExp::Vertices(edge,first,last,true);
            const auto polygon=BRep_Tool::PolygonOnTriangulation(edge,mesh,location);
            const bool degenerate=BRep_Tool::Degenerated(edge);
            catalog << " {e" << edges_.FindIndex(edge)-1 << " o" << static_cast<int>(edge.Orientation()) <<
                " deg" << degenerate << " v" << vertices_.FindIndex(first)-1 << '/' << vertices_.FindIndex(last)-1 <<
                " tol" << BRep_Tool::Tolerance(edge) << " N" << (polygon.IsNull() ? 0 : polygon->NbNodes());
            if (!degenerate) catalog << " type" << static_cast<int>(BRepAdaptor_Curve(edge).GetType());
            if (!polygon.IsNull() && mesh->HasUVNodes() && polygon->NbNodes()<=256) {
              const int ai=edge.Orientation()==TopAbs_REVERSED ? polygon->NbNodes() : 1;
              const int bi=edge.Orientation()==TopAbs_REVERSED ? 1 : polygon->NbNodes();
              const int a=polygon->Node(ai),b=polygon->Node(bi);
              if (a>=1 && b>=1 && a<=mesh->NbNodes() && b<=mesh->NbNodes()) {
                const auto x=mesh->UVNode(a),y=mesh->UVNode(b);
                catalog << " ids" << a << '/' << b << " uv" << x.X() << '/' << x.Y() << ':' << y.X() << '/' << y.Y();
                if (polygon->HasParameters()) catalog << " t" << polygon->Parameter(ai) << '/' << polygon->Parameter(bi);
              }
            }
            catalog << '}';
          }
          if (edge_count>6 || wi>=1) break;
        }
        catalog << ']';
        } catch (const Standard_Failure&) { catalog << " [face " << owner.face << " catalog OCCT exception]"; }
          catch (const std::exception&) { catalog << " [face " << owner.face << " catalog unavailable]"; }
      }
      wire_catalog=catalog.str().substr(0,2400);
      details << ". Worst native edge ownership";
      std::set<int> reported_owner_stages;
      for (const auto& edge : worst_edges) {
        if (!selected_edges.count(edge.second+1)) continue;
        for (const auto& entry : native_uses) if (entry.first.second==edge.second+1) {
          const auto& use=entry.second;
          details << " [edge " << edge.second << " shell " << entry.first.first-1 << " native-deg " <<
              BRep_Tool::Degenerated(TopoDS::Edge(edges_.FindKey(edge.second+1))) << " occurrences/balance/unique-faces " <<
              use.count << '/' << use.balance << '/' << use.faces.size() << " face/orient/POT-nodes";
          for (const auto& owner : use.owners) {
            details << " {" << owner.face << '/' << owner.orientation << '/' << owner.nodes;
            if (reported_owner_stages.insert(owner.face).second) details << " stage:" << owner.stage;
            else details << " stage:previous owner";
            details << owner.physical << '}';
          }
          details << ']';
        }
      }
    } catch (const Standard_Failure&) { details << ". Native incidence unavailable (OCCT)"; }
      catch (const std::exception&) { details << ". Native incidence unavailable (budget/native)"; }
    details << ". Restoration stages";
    int stages=0;
    std::vector<std::pair<int,int>> stage_faces;
    for (const auto& face : face_groups) stage_faces.emplace_back(face.second,face.first);
    std::sort(stage_faces.begin(),stage_faces.end(),[](const auto& a,const auto& b) { return a>b; });
    for (const auto& face : stage_faces) {
      const auto example=examples_by_face.find(face.second);
      if (example!=examples_by_face.end()) failures.push_back(example->second);
    }
    for (const auto& face : stage_faces) {
      const auto rejection=rejections_.find(face.second);
      if (rejection==rejections_.end() || rejection->second=="no mapped-native boundary shortcut" ||
          rejection->second=="native boundary restored and full domain certified") continue;
      details << " [face " << face.second << " bad uses " << face.first << ' ' << rejection->second.substr(0,360) << ']';
      if (++stages==4) break;
    }
    details << ". Source face/link-use groups";
    const auto groups=[&](const std::map<int,int>& source) {
      std::vector<std::pair<int,int>> ranked;
      for (const auto& entry : source) ranked.emplace_back(entry.second,entry.first);
      std::sort(ranked.begin(),ranked.end(),[](const auto& a,const auto& b) { return a>b; });
      for (std::size_t i=0;i<std::min<std::size_t>(12,ranked.size());++i)
        details << ' ' << ranked[i].second << ':' << ranked[i].first;
      if (ranked.size()>12) details << " [" << ranked.size()-12 << " further groups]";
    };
    groups(face_groups); details << "; native edge/endpoint-use groups"; groups(edge_groups);
    details << "; retained positive small-facet groups"; groups(retained_small_faces_);
    details << "; native extraction skipped triangle groups"; groups(skipped_faces_);
    details << "; positive threshold-skipped groups"; groups(positive_skipped_faces_);
    for (std::size_t i=0;i<skipped_facets_.size();++i) {
      const auto& f=skipped_facets_[i]; details << " [face " << f.face << " tri " << f.triangle <<
          " nodes " << f.nodes[0] << '/' << f.nodes[1] << '/' << f.nodes[2] << " cross2 " << skipped_cross2_[i] << ']';
    }
    const auto vertex=[&](std::uint32_t id) {
      const auto& key=vertex_keys_.at(id);
      details << id << "{shell " << key[0]-1 << ' ' << (key[1]==0 ? "face/node " : key[1]==1 ? "vertex " : "edge/sample ") <<
          key[2]-1 << '/' << key[3] << " tol " << vertex_tolerances_.at(id);
      if (key[1]==2) details << " t " << edge_parameters_.at({key[0],key[2]}).at(key[3]-1);
      details << " XYZ " << output.export_positions[3*id] << ',' << output.export_positions[3*id+1] << ',' <<
          output.export_positions[3*id+2] << '}';
    };
    std::size_t examples=0,raw_work=0;
    for (const auto& failure : failures) {
      if (details.str().size()>3800) break;
      const auto& use=failure.second;
      details << "; link "; vertex(failure.first.first); details << '/'; vertex(failure.first.second);
      details << " uses/balance " << use.count << '/' << use.balance;
      for (int i=0;i<std::min(2,use.count);++i) {
        const auto& incident=use.incidents[i]; const auto& f=facets_.at(incident.first);
        const int a=f.nodes[incident.second],b=f.nodes[(incident.second+1)%3];
        details << " incident face/tri/facet " << f.face << '/' << f.triangle << '/' << incident.first <<
            " native nodes " << a << '/' << b << " third " << f.nodes[(incident.second+2)%3];
        const auto& mesh=face_meshes_.at(f.face); int raw=0;
        if (mesh->HasUVNodes()) {
          const auto ua=mesh->UVNode(a),ub=mesh->UVNode(b);
          details << " UV " << ua.X() << ',' << ua.Y() << '/' << ub.X() << ',' << ub.Y();
        }
        if (raw_work+static_cast<std::size_t>(mesh->NbTriangles())<=2000000) {
          raw_work+=mesh->NbTriangles();
          for (int ti=1;ti<=mesh->NbTriangles();++ti) {
            const auto triangle=mesh->Triangle(ti);
            for (int j=1;j<=3;++j) {
              const int x=triangle.Value(j),y=triangle.Value(j%3+1);
              if ((x==a && y==b) || (x==b && y==a)) ++raw;
            }
          }
          details << " raw face uses " << raw;
        } else details << " raw incidence budget exhausted";
      }
      if (++examples==8) break;
    }
    auto failure=details.str();
    if (!wire_catalog.empty()) failure.insert(catalog_position,". Spherical constraint catalog"+wire_catalog);
    throw std::runtime_error(failure.substr(0,4800));
  }
 private:
  [[noreturn]] void fail(int face_index,const std::string& message) const {
    throw std::runtime_error("Native export topology face "+std::to_string(face_index-1)+": "+message+
        "; native shells "+std::to_string(shells_.Extent()));
  }
  TopoDS_Shape shape_;
  const TopTools_IndexedMapOfShape& edges_;
  TopTools_IndexedMapOfShape shells_,vertices_;
  TopTools_IndexedDataMapOfShapeListOfShape face_shells_;
  std::map<std::pair<int,int>,std::vector<double>> edge_parameters_;
  std::map<Key,Sample> samples_;
  std::vector<Key> vertex_keys_;
  std::vector<double> vertex_tolerances_;
  std::vector<Facet> facets_,skipped_facets_;
  std::vector<double> skipped_cross2_;
  std::map<int,int> skipped_faces_,positive_skipped_faces_,retained_small_faces_;
  std::map<int,Handle(Poly_Triangulation)> face_meshes_;
  std::string recovery_;
  std::map<int,std::string> rejections_;
  double deflection_;
  bool closed_=false;
  std::size_t node_work_=0,parameter_work_=0;
};

// Optional reporting provenance is computed once per body. Local annular
// faces also occur inside sealed chambers, so their normals cannot establish
// exterior passage without the enclosing solid's exact shell membership.
static std::vector<std::uint8_t> face_outer_shell_flags(
    const TopoDS_Shape& shape, const TopTools_IndexedMapOfShape& faces,
    const TopTools_IndexedMapOfShape& edges) {
  std::vector<std::uint8_t> flags(static_cast<std::size_t>(faces.Extent()), 0);
  try {
    if (shape.IsNull() || shape.ShapeType() != TopAbs_SOLID ||
        shape.Orientation() != TopAbs_FORWARD) return flags;
    // Reporting is optional and the recognizer handles only simple analytic
    // walls. Do not run full-solid validation on an arbitrary large/curved
    // imported model merely to replace a correctly unknown hole extent.
    if (faces.Extent() > 128 || edges.Extent() > 256) return flags;
    const auto finite_point = [](const gp_Pnt& p) {
      return std::isfinite(p.X()) && std::isfinite(p.Y()) && std::isfinite(p.Z());
    };
    for (int i = 1; i <= edges.Extent(); ++i) {
      BRepAdaptor_Curve edge(TopoDS::Edge(edges.FindKey(i)));
      if (!std::isfinite(edge.FirstParameter()) || !std::isfinite(edge.LastParameter()))
        return flags;
      if (!finite_point(edge.Value(edge.FirstParameter())) ||
          !finite_point(edge.Value(edge.LastParameter()))) return flags;
      if (edge.GetType() == GeomAbs_Line) {
        if (!finite_point(edge.Line().Location())) return flags;
      } else if (edge.GetType() == GeomAbs_Circle) {
        const gp_Circ circle = edge.Circle();
        if (!finite_point(circle.Location()) || !std::isfinite(circle.Radius()) ||
            circle.Radius() <= 0.0) return flags;
      } else return flags;
    }
    int boundary_uses = 0;
    for (int i = 1; i <= faces.Extent(); ++i) {
      const TopoDS_Face face = TopoDS::Face(faces.FindKey(i));
      BRepAdaptor_Surface surface(face, true);
      if (!std::isfinite(surface.FirstUParameter()) || !std::isfinite(surface.LastUParameter()) ||
          !std::isfinite(surface.FirstVParameter()) || !std::isfinite(surface.LastVParameter()))
        return flags;
      if (surface.GetType() == GeomAbs_Plane) {
        if (!finite_point(surface.Plane().Location())) return flags;
      } else if (surface.GetType() == GeomAbs_Cylinder) {
        const gp_Cylinder cylinder = surface.Cylinder();
        if (!finite_point(cylinder.Location()) || !std::isfinite(cylinder.Radius()) ||
            cylinder.Radius() <= 0.0) return flags;
      } else return flags;
      for (TopExp_Explorer use(face, TopAbs_EDGE); use.More(); use.Next()) {
        if (++boundary_uses > 1024) return flags;
        double first, last;
        const auto curve = BRep_Tool::CurveOnSurface(TopoDS::Edge(use.Current()), face, first, last);
        if (curve.IsNull() || !std::isfinite(first) || !std::isfinite(last)) return flags;
        const Geom2dAdaptor_Curve boundary(curve, first, last);
        if (boundary.GetType() != GeomAbs_Line && boundary.GetType() != GeomAbs_Circle)
          return flags;
        for (const double parameter : {first, first * 0.5 + last * 0.5, last}) {
          const gp_Pnt2d point = boundary.Value(parameter);
          if (!std::isfinite(point.X()) || !std::isfinite(point.Y())) return flags;
        }
      }
    }
    const TopoDS_Solid solid = TopoDS::Solid(shape);
    if (!BRepCheck_Analyzer(solid, true, false).IsValid()) return flags;
    TopTools_IndexedMapOfShape shells;
    TopExp::MapShapes(solid, TopAbs_SHELL, shells);
    if (shells.Extent() == 0) return flags;
    for (int i = 1; i <= shells.Extent(); ++i)
      if (!BRep_Tool::IsClosed(shells.FindKey(i))) return flags;
    BRepClass3d_SolidClassifier orientation(solid);
    orientation.PerformInfinitePoint(Precision::Confusion());
    if (orientation.State() != TopAbs_OUT) return flags;
    // OuterShell returns a lone shell without checking its orientation;
    // validity, closure and the infinite-point check above are all required.
    const TopoDS_Shell outer = BRepClass3d::OuterShell(solid);
    if (outer.IsNull()) return flags;
    TopTools_IndexedMapOfShape exterior_faces;
    TopExp::MapShapes(outer, TopAbs_FACE, exterior_faces);
    for (int i = 1; i <= faces.Extent(); ++i)
      flags[static_cast<std::size_t>(i - 1)] = exterior_faces.Contains(faces.FindKey(i)) ? 1 : 2;
  } catch (const Standard_Failure&) {
    std::fill(flags.begin(), flags.end(), 0);
  } catch (const std::exception&) {
    std::fill(flags.begin(), flags.end(), 0);
  }
  // Unsupported/invalid geometry keeps its original scene and unknown evidence.
  return flags;
}

static FfiMesh mesh_shape(std::uint64_t body_id,
                          const TopoDS_Shape& shape,
                          double linear_deflection,
                          double angular_deflection,
                          bool imported_display = false,
                          const SectionMeshBudget* budget = nullptr,
                          const Message_ProgressRange& range = Message_ProgressRange(),
                          bool native_export_precision = false);

// Shared exact clipping for drawing projections and disposable 3D inspection.
static TopoDS_Shape retain_half_space(const TopoDS_Shape& source,
                                     const gp_Pln& boundary,
                                     const gp_Pnt& retained_point,
                                     const Message_ProgressRange& range = Message_ProgressRange()) {
  const TopoDS_Face face = BRepBuilderAPI_MakeFace(boundary).Face();
  const TopoDS_Solid half_space =
      BRepPrimAPI_MakeHalfSpace(face, retained_point).Solid();
  BRepAlgoAPI_Common common;
  TopTools_ListOfShape arguments, tools;
  arguments.Append(source);
  tools.Append(half_space);
  common.SetArguments(arguments);
  common.SetTools(tools);
  common.SetNonDestructive(true);
  common.Build(range);
  if (!common.IsDone() || common.HasErrors()) {
    std::ostringstream errors;
    common.DumpErrors(errors);
    throw std::runtime_error("OCCT section clipping failed: " + errors.str());
  }
  return common.Shape();
}

static TopoDS_Shape exact_section_shape(const TopoDS_Shape& source,
                                      const gp_Pln& plane,
                                      const Message_ProgressRange& range = Message_ProgressRange()) {
  BRepAlgoAPI_Section section(source, plane, false);
  section.SetNonDestructive(true);
  section.Approximation(true);
  section.Build(range);
  if (!section.IsDone() || section.HasErrors()) {
    std::ostringstream errors;
    section.DumpErrors(errors);
    throw std::runtime_error("OCCT section intersection failed: " + errors.str());
  }
  return section.Shape();
}

// Section edges alone contain coplanar exterior-face terminations and seams.
// Material hatching instead uses the regularized plane / 3D-interior region.
// Its boundary is independent of which 3D half the caller retains.
struct SectionRegionBudget {
  std::size_t topology;
  std::size_t comparisons = 16 * 1024 * 1024;
  const SectionProgress* progress;

  void take(const char* stage, std::size_t count = 1) {
    if (progress) progress->check(stage);
    if (count > topology) {
      throw std::runtime_error(std::string("Section material topology exceeds the native budget during ") + stage);
    }
    topology -= count;
  }

  void boolean_work(std::size_t arguments, std::size_t tools, const char* stage) {
    if (progress) progress->check(stage);
    if (arguments > std::numeric_limits<std::size_t>::max() - tools) {
      throw std::runtime_error("Section material Boolean input exceeds the native budget");
    }
    const auto count = arguments + tools;
    // Conservatively bound potential pair work, including within each operand,
    // before entering an OCCT operation. One allowance spans the whole query.
    if (count != 0 && count > comparisons / count) {
      throw std::runtime_error(std::string("Section material Boolean exceeds the native comparison budget during ") + stage);
    }
    comparisons -= count * count;
  }
};

static std::size_t section_shape_complexity(const TopoDS_Shape& shape,
                                            SectionRegionBudget& budget,
                                            const char* stage) {
  std::size_t count = 0;
  if (shape.IsNull()) return count;
  for (const auto kind : {TopAbs_FACE, TopAbs_EDGE, TopAbs_VERTEX}) {
    TopTools_IndexedMapOfShape seen;
    for (TopExp_Explorer explorer(shape, kind); explorer.More(); explorer.Next()) {
      budget.take(stage); // Bound traversal as well as unique storage growth.
      if (seen.Contains(explorer.Current())) continue;
      budget.take(stage);
      seen.Add(explorer.Current());
      ++count;
    }
  }
  return count;
}

struct SectionFaces {
  TopoDS_Compound shape;
  std::size_t count = 0;
  std::size_t complexity = 0;
};

static SectionFaces section_faces_only(const TopoDS_Shape& shape,
                                       SectionRegionBudget& budget,
                                       const char* stage) {
  SectionFaces result;
  budget.take(stage);
  BRep_Builder builder;
  builder.MakeCompound(result.shape);
  if (shape.IsNull()) return result;
  TopTools_IndexedMapOfShape faces;
  for (TopExp_Explorer explorer(shape, TopAbs_FACE); explorer.More(); explorer.Next()) {
    budget.take(stage);
    if (faces.Contains(explorer.Current())) continue;
    budget.take(stage, 2); // Face map and compound member, before either grows.
    faces.Add(explorer.Current());
    builder.Add(result.shape, explorer.Current());
    ++result.count;
  }
  if (result.count != 0) {
    result.complexity = section_shape_complexity(result.shape, budget, stage);
  }
  return result;
}

template <typename Boolean>
static TopoDS_Shape section_region_boolean(const TopoDS_Shape& source,
                                           std::size_t source_complexity,
                                           const TopoDS_Shape& tool,
                                           std::size_t tool_complexity,
                                           SectionRegionBudget& budget,
                                           const char* stage,
                                           const Message_ProgressRange& range) {
  budget.boolean_work(source_complexity, tool_complexity, stage);
  Boolean operation;
  TopTools_ListOfShape arguments, tools;
  arguments.Append(source);
  tools.Append(tool);
  operation.SetArguments(arguments);
  operation.SetTools(tools);
  operation.SetNonDestructive(true);
  operation.Build(range);
  if (budget.progress) budget.progress->check(stage);
  if (!operation.IsDone() || operation.HasErrors()) {
    std::ostringstream errors;
    operation.DumpErrors(errors);
    throw std::runtime_error(std::string("OCCT section material ") + stage + " failed: " + errors.str());
  }
  return operation.Shape();
}

static bool section_face_may_have_planar_area(const TopoDS_Face& face) {
  const auto finite_frame = [](const gp_Ax3& frame) {
    const auto& origin = frame.Location();
    if (!std::isfinite(origin.X()) || !std::isfinite(origin.Y()) ||
        !std::isfinite(origin.Z())) return false;
    for (const auto& direction : {frame.Direction(), frame.XDirection(), frame.YDirection()}) {
      if (!std::isfinite(direction.X()) || !std::isfinite(direction.Y()) ||
          !std::isfinite(direction.Z())) return false;
    }
    return true;
  };
  // These finite, nondegenerate analytic supports contain no open planar
  // patch. General/unknown supports (including planar splines) must still
  // undergo exact Common; sampling or an origin/normal test is not proof.
  try {
    const BRepAdaptor_Surface surface(face, true);
    switch (surface.GetType()) {
      case GeomAbs_Cylinder: {
        const auto cylinder = surface.Cylinder();
        return !(finite_frame(cylinder.Position()) && std::isfinite(cylinder.Radius()) &&
                 cylinder.Radius() > 0.);
      }
      case GeomAbs_Cone: {
        const auto cone = surface.Cone();
        const auto angle = std::abs(cone.SemiAngle());
        return !(finite_frame(cone.Position()) && std::isfinite(cone.RefRadius()) &&
                 cone.RefRadius() >= 0. && std::isfinite(angle) && angle > 0. && angle < kPi * 0.5);
      }
      case GeomAbs_Sphere: {
        const auto sphere = surface.Sphere();
        return !(finite_frame(sphere.Position()) && std::isfinite(sphere.Radius()) &&
                 sphere.Radius() > 0.);
      }
      case GeomAbs_Torus: {
        const auto torus = surface.Torus();
        return !(finite_frame(torus.Position()) && std::isfinite(torus.MajorRadius()) &&
                 std::isfinite(torus.MinorRadius()) && torus.MinorRadius() > 0. &&
                 torus.MajorRadius() > torus.MinorRadius());
      }
      default: return true;
    }
  } catch (const Standard_OutOfMemory&) {
    throw;
  } catch (const Standard_Failure&) {
    // Eligibility is an optional optimization. If introspection cannot prove
    // exclusion, retain the exact Boolean path. Native memory exhaustion and
    // budget failures outside this try block remain fatal.
    return true;
  }
}

static SectionFaces section_boundary_contact(const TopoDS_Shape& solid,
                                              const TopoDS_Face& tool,
                                              SectionRegionBudget& budget,
                                              const Message_ProgressRange& range) {
  TopTools_IndexedMapOfShape faces, contact_faces;
  for (TopExp_Explorer explorer(solid, TopAbs_FACE); explorer.More(); explorer.Next()) {
    budget.take("source boundary face traversal");
    if (explorer.Current().Orientation() == TopAbs_INTERNAL ||
        explorer.Current().Orientation() == TopAbs_EXTERNAL ||
        faces.Contains(explorer.Current())) continue;
    budget.take("source boundary face storage");
    faces.Add(explorer.Current());
  }
  SectionFaces contact;
  budget.take("coplanar boundary compound");
  BRep_Builder builder;
  builder.MakeCompound(contact.shape);
  Message_ProgressScope members(range, "Coplanar boundary faces", faces.Extent());
  for (int index = 1; index <= faces.Extent(); ++index) {
    const auto face_range = members.Next();
    const auto face = TopoDS::Face(faces.FindKey(index));
    const auto complexity = section_shape_complexity(face, budget, "boundary face input");
    if (!section_face_may_have_planar_area(face)) continue;
    // Intersection distributes over the boundary-face union. Querying each
    // face independently avoids unrelated source-face interference work;
    // every actual Boolean still shares the original comparison allowance.
    const auto common = section_region_boolean<BRepAlgoAPI_Common>(
        face, complexity, tool, 9, budget, "coplanar boundary common", face_range);
    if (common.IsNull()) continue;
    for (TopExp_Explorer explorer(common, TopAbs_FACE); explorer.More(); explorer.Next()) {
      budget.take("coplanar boundary result traversal");
      if (contact_faces.Contains(explorer.Current())) continue;
      budget.take("coplanar boundary result storage", 2);
      contact_faces.Add(explorer.Current());
      builder.Add(contact.shape, explorer.Current());
      ++contact.count;
    }
  }
  if (contact.count != 0) {
    contact.complexity = section_shape_complexity(contact.shape, budget, "coplanar boundary faces");
  }
  return contact;
}

static TopoDS_Face bounded_section_face(const TopoDS_Shape& source,
                                        const gp_Pln& plane,
                                        SectionRegionBudget& budget) {
  budget.take("plane bounds");
  Bnd_Box bounds;
  BRepBndLib::Add(source, bounds, false);
  if (bounds.IsVoid() || bounds.IsOpen()) {
    throw std::runtime_error("Section material requires finite source bounds");
  }
  double low[3], high[3];
  bounds.Get(low[0], low[1], low[2], high[0], high[1], high[2]);
  const gp_Vec u(plane.Position().XDirection());
  const gp_Vec v(plane.Position().YDirection());
  double u_min = std::numeric_limits<double>::infinity();
  double v_min = u_min;
  double u_max = -u_min;
  double v_max = -u_min;
  for (int corner = 0; corner < 8; ++corner) {
    const gp_Pnt point((corner & 1) ? high[0] : low[0],
                       (corner & 2) ? high[1] : low[1],
                       (corner & 4) ? high[2] : low[2]);
    const gp_Vec relative(plane.Location(), point);
    const double pu = relative.Dot(u), pv = relative.Dot(v);
    u_min = std::min(u_min, pu); u_max = std::max(u_max, pu);
    v_min = std::min(v_min, pv); v_max = std::max(v_max, pv);
  }
  const double extent = std::max(u_max - u_min, v_max - v_min);
  if (!std::isfinite(extent) || extent <= 0.) {
    throw std::runtime_error("Section material plane bounds are degenerate");
  }
  // Expand only the finite tool's in-plane edges, never the cutting plane.
  const double margin = std::max(1.0, extent * 1e-6);
  if (!std::isfinite(u_min - margin) || !std::isfinite(u_max + margin) ||
      !std::isfinite(v_min - margin) || !std::isfinite(v_max + margin)) {
    throw std::runtime_error("Section material plane bounds exceed the finite range");
  }
  budget.take("bounded plane storage", 9); // One face, four edges, four vertices.
  BRepBuilderAPI_MakeFace face(plane, u_min - margin, u_max + margin,
                             v_min - margin, v_max + margin);
  if (!face.IsDone()) throw std::runtime_error("OCCT bounded section plane failed");
  return face.Face();
}

static TopoDS_Shape material_section_boundary(const TopoDS_Shape& source,
                                              const gp_Pln& plane,
                                              SectionRegionBudget& budget,
                                              const Message_ProgressRange& range) {
  TopTools_IndexedMapOfShape solids;
  for (TopExp_Explorer explorer(source, TopAbs_SOLID); explorer.More(); explorer.Next()) {
    budget.take("material source solids");
    if (solids.Contains(explorer.Current())) continue;
    budget.take("material source solids");
    solids.Add(explorer.Current());
  }
  if (solids.IsEmpty()) {
    // Open imported surfaces have visible intersections but no 3D interior.
    // Drawing callers keep those as contact lines; inspection separately
    // retains its solid-only requirement before calling this helper.
    return TopoDS_Shape();
  }
  const auto tool = bounded_section_face(source, plane, budget);
  Message_ProgressScope stages(range, "Planar material regions", solids.Extent() + 1);
  std::vector<SectionFaces> regions;
  for (int index = 1; index <= solids.Extent(); ++index) {
    Message_ProgressScope member(stages.Next(), "Source solid material", 3);
    const auto& solid = solids.FindKey(index);
    const auto complexity = section_shape_complexity(solid, budget, "solid material input");
    auto material = section_faces_only(section_region_boolean<BRepAlgoAPI_Common>(
        solid, complexity, tool, 9, budget, "plane common", member.Next()),
        budget, "plane common faces");
    if (material.count == 0) continue;
    const auto contact = section_boundary_contact(solid, tool, budget, member.Next());
    if (contact.count != 0) {
      material = section_faces_only(section_region_boolean<BRepAlgoAPI_Cut>(
          material.shape, material.complexity, contact.shape, contact.complexity,
          budget, "exterior face subtraction", member.Next()), budget, "strict interior faces");
    }
    // Removing exterior patches must be per solid: another compound member
    // may have genuine interior material in the same patch of the plane.
    if (material.count != 0) {
      budget.take("material region storage");
      regions.push_back(std::move(material));
    }
  }
  if (regions.empty()) return TopoDS_Shape();
  TopoDS_Shape material = regions.front().shape;
  if (regions.size() > 1) {
    TopTools_ListOfShape arguments, tools;
    budget.take("material union operands");
    arguments.Append(material);
    std::size_t tool_complexity = 0;
    for (std::size_t index = 1; index < regions.size(); ++index) {
      budget.take("material union operands");
      tools.Append(regions[index].shape);
      tool_complexity += regions[index].complexity; // Bounded by topology allowance.
    }
    budget.boolean_work(regions.front().complexity, tool_complexity, "material region union");
    BRepAlgoAPI_Fuse unite;
    unite.SetArguments(arguments);
    unite.SetTools(tools);
    unite.SetNonDestructive(true);
    unite.Build(stages.Next());
    if (budget.progress) budget.progress->check("material region union");
    if (!unite.IsDone() || unite.HasErrors()) {
      std::ostringstream errors;
      unite.DumpErrors(errors);
      throw std::runtime_error("OCCT section material union failed: " + errors.str());
    }
    material = section_faces_only(unite.Shape(), budget, "material union faces").shape;
  }

  // Boolean face regions already split shared boundaries exactly. Incidence
  // removes only their topological internal seams, without a geometric unifier
  // or suppression of unclosed/ambiguous section curves.
  TopTools_IndexedMapOfShape faces, edges;
  struct BoundaryIncidence {
    unsigned count;
    TopAbs_Orientation orientation;
  };
  std::vector<BoundaryIncidence> incidence;
  for (TopExp_Explorer face_explorer(material, TopAbs_FACE);
       face_explorer.More(); face_explorer.Next()) {
    budget.take("material boundary faces");
    if (faces.Contains(face_explorer.Current())) continue;
    budget.take("material boundary faces");
    faces.Add(face_explorer.Current());
    auto face = TopoDS::Face(face_explorer.Current());
    const auto face_complexity = section_shape_complexity(face, budget, "material face validation input");
    budget.boolean_work(face_complexity, 0, "material face topology validation");
    if (!BRepCheck_Analyzer(face, false).IsValid()) {
      throw std::runtime_error("OCCT section material face has invalid boundary topology");
    }
    if (budget.progress) budget.progress->check("material face topology validation");
    const BRepAdaptor_Surface surface(face, true);
    if (surface.GetType() != GeomAbs_Plane) {
      throw std::runtime_error("OCCT section material region is not planar");
    }
    const auto surface_plane = surface.Plane();
    const double normal_dot = gp_Vec(parametric_plane_normal(surface_plane)).Dot(
        gp_Vec(plane.Axis().Direction()));
    const double distance = plane.Distance(surface_plane.Location());
    const double tolerance = BRep_Tool::Tolerance(face);
    if (!std::isfinite(normal_dot) || !std::isfinite(distance) || !std::isfinite(tolerance) ||
        std::abs(normal_dot) < 1. - Precision::Angular() ||
        distance > tolerance + Precision::Confusion()) {
      throw std::runtime_error("OCCT section material region does not lie on the requested plane");
    }
    // Normalize only this derived TopoDS wrapper's orientation. With all face
    // normals aligned, a genuine shared seam has two opposite oriented uses.
    face.Orientation(normal_dot >= 0. ? TopAbs_FORWARD : TopAbs_REVERSED);
    TopTools_IndexedMapOfShape face_edges;
    for (TopExp_Explorer wire_explorer(face, TopAbs_WIRE);
         wire_explorer.More(); wire_explorer.Next()) {
      budget.take("material boundary wires");
      const auto wire = TopoDS::Wire(wire_explorer.Current());
      if (wire.Orientation() == TopAbs_INTERNAL || wire.Orientation() == TopAbs_EXTERNAL) continue;
      for (TopExp_Explorer edge_explorer(wire, TopAbs_EDGE);
           edge_explorer.More(); edge_explorer.Next()) {
        budget.take("material boundary edges");
        const auto edge = TopoDS::Edge(edge_explorer.Current());
        // Ordered WireExplorer can stop at an unoriented use. Incidence needs
        // every oriented boundary use, but no traversal order or internal marks.
        if (edge.Orientation() == TopAbs_INTERNAL || edge.Orientation() == TopAbs_EXTERNAL) continue;
        if (face_edges.Contains(edge)) {
          throw std::runtime_error("OCCT section material boundary repeats an edge in a planar face");
        }
        budget.take("material boundary edge storage");
        face_edges.Add(edge);
        int edge_index = edges.FindIndex(edge);
        if (edge_index == 0) {
          budget.take("material boundary edge storage", 2);
          edge_index = edges.Add(edge);
          incidence.push_back({0, edge.Orientation()});
        }
        auto& use = incidence[static_cast<std::size_t>(edge_index - 1)];
        if (++use.count > 2) {
          throw std::runtime_error("OCCT section material boundary is non-manifold");
        }
        if (use.count == 2 && use.orientation == edge.Orientation()) {
          throw std::runtime_error("OCCT section material shared boundary has ambiguous orientation");
        }
      }
    }
  }
  budget.take("material boundary output");
  BRep_Builder builder;
  TopoDS_Compound result;
  builder.MakeCompound(result);
  bool has_boundary = false;
  for (int index = 1; index <= edges.Extent(); ++index) {
    budget.take("material boundary incidence");
    if (incidence[static_cast<std::size_t>(index - 1)].count != 1) continue;
    budget.take("material boundary output");
    builder.Add(result, edges.FindKey(index));
    has_boundary = true;
  }
  if (!has_boundary) {
    throw std::runtime_error("OCCT section material region has no boundary");
  }
  return result;
}

FfiSectionGeometry Kernel::section_geometry(std::uint64_t body_id,
                                            const FfiSectionOptions& options) const {
  const auto found = impl_->bodies.find(body_id);
  const auto axis = options.axis;
  const auto offset = options.offset;
  if (found == impl_->bodies.end() || axis > 2 || !std::isfinite(offset) ||
      !std::isfinite(options.deflection) || options.deflection < 0.001 ||
      options.deflection > 0.1 || options.timeout_ms > 30'000) {
    throw std::runtime_error("Invalid body or section plane");
  }
  Handle(SectionProgress) progress = new SectionProgress(options.timeout_ms);
  progress->check("dispatch");
  Message_ProgressScope stages(progress->Start(), "Section inspection", 4);
  SectionRegionBudget region_budget{options.contour_points, 16 * 1024 * 1024, progress.get()};
  const auto source_complexity = section_shape_complexity(found->second, region_budget,
                                                          "section source topology");
  double coordinates[3] = {0., 0., 0.};
  coordinates[axis] = offset;
  const gp_Pnt point(coordinates[0], coordinates[1], coordinates[2]);
  double normal[3] = {0., 0., 0.};
  normal[axis] = 1.;
  const gp_Vec direction(normal[0], normal[1], normal[2]);
  const gp_Pln plane(point, gp_Dir(direction));
  region_budget.boolean_work(source_complexity, 1, "section intersection");
  const TopoDS_Shape section = exact_section_shape(found->second, plane, stages.Next());
  progress->check("intersection");
  const gp_Vec right = axis == 0 ? gp_Vec(0., 1., 0.) : gp_Vec(1., 0., 0.);
  const gp_Vec up = axis == 2 ? gp_Vec(0., 1., 0.) : gp_Vec(0., 0., 1.);
  FfiSectionGeometry output;
  output.outcome = 0;
  output.has_cutaway = false;
  output.cutaway.body_id = body_id;
  output.offsets.push_back(0);
  if (!TopExp_Explorer(section, TopAbs_EDGE).More()) {
    output.outcome = TopExp_Explorer(section, TopAbs_VERTEX).More() ? 1 : 0;
    return output;
  }

  TopoDS_Shape source = found->second;
  if (options.include_cutaway) {
    region_budget.take("cutaway source copy", source_complexity);
    BRepBuilderAPI_Copy copy(source, true, false);
    if (!copy.IsDone() || copy.Shape().IsNull()) {
      throw std::runtime_error("OCCT section shape copy failed");
    }
    source = copy.Shape();
  }
  TopTools_IndexedMapOfShape solids;
  for (TopExp_Explorer explorer(source, TopAbs_SOLID); explorer.More(); explorer.Next()) {
    progress->check("solid classification");
    if (static_cast<std::size_t>(solids.Extent()) >= options.contour_points) {
      throw std::runtime_error("Section source exceeds the native solid budget");
    }
    solids.Add(explorer.Current());
  }
  if (solids.IsEmpty()) throw std::runtime_error("Section inspection requires solid geometry");
  Message_ProgressScope clipping(stages.Next(), "Clip section solids", solids.Extent());
  BRep_Builder builder;
  TopoDS_Compound clipped;
  builder.MakeCompound(clipped);
  bool splits_material = false;
  for (int index = 1; index <= solids.Extent(); ++index) {
    const auto& solid = solids.FindKey(index);
    const auto complexity = section_shape_complexity(solid, region_budget, "section clipping input");
    region_budget.boolean_work(complexity, 1, "section clipping");
    const auto retained = retain_half_space(solid, plane,
        point.Translated(direction.Multiplied(options.keep_positive ? 1. : -1.)), clipping.Next());
    progress->check("clipping");
    GProp_GProps source_properties, retained_properties;
    BRepGProp::VolumeProperties(solid, source_properties);
    if (!retained.IsNull()) BRepGProp::VolumeProperties(retained, retained_properties);
    const double source_volume = std::abs(source_properties.Mass());
    const double retained_volume = std::abs(retained_properties.Mass());
    if (!std::isfinite(source_volume) || source_volume <= 0.0 || !std::isfinite(retained_volume)) {
      throw std::runtime_error("Section inspection requires a finite solid volume");
    }
    const double tolerance = std::max(1e-15, source_volume * 1e-12);
    splits_material |= retained_volume > tolerance && source_volume - retained_volume > tolerance;
    if (retained_volume > tolerance) builder.Add(clipped, retained);
  }
  output.outcome = splits_material ? 2 : 1;
  const TopoDS_Shape boundaries = output.outcome == 2
      ? material_section_boundary(found->second, plane, region_budget, stages.Next())
      : section;
  if (output.outcome == 2 && boundaries.IsNull()) {
    throw std::runtime_error("OCCT section split source volume but produced no strict interior material region");
  }
  std::set<std::vector<std::int64_t>> seen;
  append_section_shape(boundaries, right, up, options.deflection, output.offsets,
                       output.points, seen, progress.get(), options.contour_points);
  if (output.outcome == 2 && output.points.empty()) {
    throw std::runtime_error("OCCT section material boundary produced no contour points");
  }
  if (output.outcome == 2 && options.include_cutaway) {
    const SectionMeshBudget budget{options.vertices, options.edge_points, progress.get()};
    output.cutaway = mesh_shape(body_id, clipped, options.deflection, 0.25, false, &budget, stages.Next());
    progress->check("meshing");
    if (output.cutaway.indices.empty()) {
      throw std::runtime_error("OCCT produced no triangles for the retained section solid");
    }
    output.has_cutaway = true;
  }
  return output;
}

FfiMesh Kernel::mesh(std::uint64_t body_id) const {
  const auto found = impl_->bodies.find(body_id);
  if (found == impl_->bodies.end()) {
    throw std::runtime_error("body is missing");
  }

  return mesh_shape(body_id, found->second, 0.15, 0.35,
      impl_->imported_display_bodies.count(body_id) != 0);
}

FfiMesh Kernel::mesh_with_deflection(
    std::uint64_t body_id,
    double linear_deflection,
    double angular_deflection) const {
  const auto found = impl_->bodies.find(body_id);
  if (found == impl_->bodies.end()) {
    throw std::runtime_error("body is missing");
  }




  BRepBuilderAPI_Copy copy(found->second, true, false);
  if (!copy.IsDone() || copy.Shape().IsNull()) {
    throw std::runtime_error("OCCT export shape copy failed");
  }
  return mesh_shape(body_id, copy.Shape(), linear_deflection,
                    angular_deflection, false, nullptr, Message_ProgressRange(), true);
}

static FfiMesh mesh_shape(std::uint64_t body_id,
                          const TopoDS_Shape& shape,
                          double linear_deflection,
                          double angular_deflection,
                          bool imported_display,
                          const SectionMeshBudget* budget,
                          const Message_ProgressRange& range,
                          bool native_export_precision) {
  const double linear =
      linear_deflection > 0.0 ? linear_deflection : 0.15;
  const double angular =
      angular_deflection > 0.0 ? angular_deflection : 0.35;
  BRepMesh_IncrementalMesh mesher;
  mesher.SetShape(shape);
  mesher.ChangeParameters().Deflection = linear;
  mesher.ChangeParameters().Angle = angular;
  // Native local edge-size scaling refines small export curves without
  // changing the requested linear/angular precision or display defaults.
  mesher.ChangeParameters().AdjustMinSize = native_export_precision;
  mesher.ChangeParameters().InParallel = true;
  auto* boundary_context = new TangentBoundaryMeshContext();
  boundary_context->EnableNativeExportRecovery(native_export_precision);
  Handle(IMeshTools_Context) context = boundary_context;
  mesher.Perform(context, range);
  if (budget) budget->progress->check("meshing");

  FfiMesh output;
  output.body_id = body_id;
  output.topology_signature = topology_signature(shape);
  TopTools_IndexedMapOfShape face_map;
  TopExp::MapShapes(shape, TopAbs_FACE, face_map);
  TopTools_IndexedMapOfShape edge_map;
  TopExp::MapShapes(shape, TopAbs_EDGE, edge_map);
  if (budget && (static_cast<std::size_t>(face_map.Extent()) > budget->vertices ||
                 static_cast<std::size_t>(edge_map.Extent()) > budget->edge_points)) {
    throw std::runtime_error("Section topology exceeds the native geometry budget");
  }
  // Disposable section/export meshes do not need this optional scene-summary
  // evidence. Keep full-solid analysis out of their bounded recovery paths.
  const auto outer_shell_flags = (budget || native_export_precision)
      ? std::vector<std::uint8_t>(static_cast<std::size_t>(face_map.Extent()), 0)
      : face_outer_shell_flags(shape, face_map, edge_map);
  for (int face_index = 1; face_index <= face_map.Extent(); ++face_index) {
    if (budget) budget->progress->check("mesh validation");
    const TopoDS_Face face = TopoDS::Face(face_map.FindKey(face_index));
    TopLoc_Location location;
    const Handle(Poly_Triangulation) triangulation =
        BRep_Tool::Triangulation(face, location);
    if (triangulation.IsNull() || triangulation->NbTriangles() == 0 || triangulation->NbNodes() < 3) {
      GProp_GProps properties;
      BRepGProp::SurfaceProperties(face, properties);
      if (!std::isfinite(properties.Mass()) || std::abs(properties.Mass()) > 1e-14) {
        std::ostringstream diagnostic;
        diagnostic.precision(17);
        diagnostic << "area " << properties.Mass()
                   << ", mesh status " << mesher.GetStatusFlags();
        const auto& model = context->GetModel();
        if (!model.IsNull()) {
          for (int fi = 0; fi < model->FacesNb(); ++fi) {
            const auto& discrete_face = model->GetFace(fi);
            if (!discrete_face->GetFace().IsSame(face)) continue;
            const auto surface_name = [](GeomAbs_SurfaceType type) {
              switch (type) {
                case GeomAbs_Plane: return "plane";
                case GeomAbs_Cylinder: return "cylinder";
                case GeomAbs_Cone: return "cone";
                case GeomAbs_Sphere: return "sphere";
                case GeomAbs_Torus: return "torus";
                case GeomAbs_BezierSurface: return "Bezier";
                case GeomAbs_BSplineSurface: return "B-spline";
                case GeomAbs_SurfaceOfRevolution: return "revolution";
                case GeomAbs_SurfaceOfExtrusion: return "extrusion";
                case GeomAbs_OffsetSurface: return "offset";
                default: return "other";
              }
            };
            diagnostic << ", surface "
                       << surface_name(discrete_face->GetSurface()->GetType())
                       << ", face status " << discrete_face->GetStatusMask()
                       << ", wires " << discrete_face->WiresNb()
                       << ", boundary repair " << boundary_context->BoundaryRepairStop();
            int reported_edges = 0;
            for (int wi = 0; wi < std::min(2, discrete_face->WiresNb()); ++wi) {
              const auto& wire = discrete_face->GetWire(wi);
              diagnostic << ", wire " << wi << " status " << wire->GetStatusMask()
                         << " edges " << wire->EdgesNb() << " samples";
              for (int ei = 0; ei < wire->EdgesNb() && reported_edges < 6;
                   ++ei, ++reported_edges) {
                const auto& pcurve = wire->GetEdge(ei)->GetPCurve(
                    discrete_face.get(), wire->GetEdgeOrientation(ei));
                diagnostic << ' ' << (pcurve.IsNull() ? 0 : pcurve->ParametersNb());
              }
            }
            if (!imported_display) diagnostic << face_mesh_failure_detail(discrete_face, context->GetParameters())
                       << spherical_boundary_failure_detail(discrete_face)
                       << boundary_failure_detail(discrete_face, context->GetParameters());
            break;
          }
        }
        const std::string failure = "OCCT did not triangulate body " +
            std::to_string(body_id) + " face " + std::to_string(face_index - 1) +
            " (" + diagnostic.str() + ")";
        if (!imported_display) throw std::runtime_error(failure);
        output.display_warning_face_indices.push_back(static_cast<std::uint32_t>(face_index - 1));
        output.display_warning_messages.push_back(rust::String(
            "Face " + std::to_string(face_index - 1) + ": display triangles missing; exact STEP retained. " +
            boundary_context->StripRepairStop(face_index - 1) + ". " + failure.substr(0, 700)));
      }
    }
  }
  std::unique_ptr<NativeExportIndex> export_index;
  if (native_export_precision) export_index=std::make_unique<NativeExportIndex>(shape,edge_map,linear,
      boundary_context->ExportBoundaryRepairStop(),boundary_context->ExportBoundaryRejections());
  context->ChangeParameters().CleanModel = true;
  context->Clean();
  context.Nullify();
  output.face_edge_offsets.push_back(0);
  for (int face_index = 1; face_index <= face_map.Extent(); ++face_index) {
    if (budget) budget->progress->check("mesh extraction");
    const TopoDS_Face face = TopoDS::Face(face_map.FindKey(face_index));
    // Exact face slots and boundary keys survive absent display triangles.
    append_plane(output.face_plane_data, face);
    append_face_signature(output.face_signature_data, face);
    append_cylinder(output.face_cylinder_data, face);
    output.face_outer_shell.push_back(outer_shell_flags[static_cast<std::size_t>(face_index - 1)]);
    BRepAdaptor_Surface surface(face, true);
    if (surface.GetType() == GeomAbs_Cone) {
      const gp_Cone cone = surface.Cone();
      output.face_cone_data.push_back(1.0);
      output.face_cone_data.push_back(cone.Axis().Direction().X());
      output.face_cone_data.push_back(cone.Axis().Direction().Y());
      output.face_cone_data.push_back(cone.Axis().Direction().Z());
      output.face_cone_data.push_back(cone.SemiAngle());
    } else {
      for (int i = 0; i < 5; ++i) output.face_cone_data.push_back(0.0);
    }
    TopTools_IndexedMapOfShape boundary;
    TopExp::MapShapes(face, TopAbs_EDGE, boundary);
    for (int i = 1; i <= boundary.Extent(); ++i) {
      const int index = edge_map.FindIndex(boundary.FindKey(i));
      if (index <= 0) throw std::runtime_error("face boundary edge is absent from body topology");
      output.face_edge_indices.push_back(static_cast<std::uint32_t>(index - 1));
      const TopoDS_Edge edge = TopoDS::Edge(boundary.FindKey(i));
      const bool linear_seam = !BRep_Tool::Degenerated(edge) &&
          BRep_Tool::IsClosed(edge, face) && BRepAdaptor_Curve(edge).GetType() == GeomAbs_Line;
      output.face_edge_linear_seams.push_back(linear_seam ? 1 : 0);
    }
    output.face_edge_offsets.push_back(static_cast<std::uint32_t>(output.face_edge_indices.size()));
    TopLoc_Location location;
    const Handle(Poly_Triangulation) triangulation =
        BRep_Tool::Triangulation(face, location);
    if (triangulation.IsNull() || triangulation->NbTriangles() == 0 || triangulation->NbNodes() < 3) {
      output.face_first_indices.push_back(static_cast<std::uint32_t>(output.indices.size()));
      output.face_index_counts.push_back(0);
      continue;
    }
    if (budget && (static_cast<std::size_t>(triangulation->NbNodes()) > budget->vertices ||
        static_cast<std::size_t>(triangulation->NbTriangles()) * 3 >
          budget->vertices - output.positions.size() / 3)) {
      throw std::runtime_error("Section mesh exceeds the native vertex budget");
    }
    if (!triangulation->HasNormals()) {
      triangulation->ComputeNormals();
    }
    output.face_first_indices.push_back(
        static_cast<std::uint32_t>(output.indices.size()));
    const gp_Trsf transform = location.Transformation();
    std::vector<NativeExportIndex::Node> export_nodes;
    if (export_index) export_nodes=export_index->face_nodes(face,face_index,triangulation,location);
    for (int triangle_index = 1;
         triangle_index <= triangulation->NbTriangles(); ++triangle_index) {
      const Poly_Triangle triangle = triangulation->Triangle(triangle_index);
      int indices[3] = {triangle.Value(1), triangle.Value(2),
                        triangle.Value(3)};
      if (face.Orientation() == TopAbs_REVERSED) {
        std::swap(indices[1], indices[2]);
      }
      gp_Pnt points[3] = {triangulation->Node(indices[0]).Transformed(transform),
                          triangulation->Node(indices[1]).Transformed(transform),
                          triangulation->Node(indices[2]).Transformed(transform)};
      gp_Vec triangle_normal(points[0], points[1]);
      triangle_normal.Cross(gp_Vec(points[0], points[2]));
      const double native_cross2=triangle_normal.SquareMagnitude();
      if (export_index && !std::isfinite(native_cross2))
        throw std::runtime_error("Native export topology face "+std::to_string(face_index-1)+
            " triangle "+std::to_string(triangle_index)+": nonfinite native facet cross product");
      // Lossless export retains every finite positive-area native facet.
      // Display keeps its existing area cutoff; exact-zero facets add no surface.
      if (export_index ? native_cross2==0.0 : native_cross2<=1e-24) {
        if (export_index) export_index->facet(face_index,triangle_index,indices,true,native_cross2);
        continue;
      }
      if (export_index && native_cross2<=1e-24) export_index->retained_small(face_index);
      if (export_index) export_index->facet(face_index,triangle_index,indices,false);
      gp_Pnt indexed_points[3]; bool indexed_changed=false;
      for (int vertex = 0; vertex < 3; ++vertex) {
        gp_Dir normal = triangulation->Normal(indices[vertex]);
        normal.Transform(transform);
        if (face.Orientation() == TopAbs_REVERSED) {
          normal.Reverse();
        }
        if (export_index) {
          const auto sample=export_index->sample(export_nodes[indices[vertex]],points[vertex],normal,output,face_index);
          output.indices.push_back(sample.index);
          indexed_points[vertex]=sample.point;
          indexed_changed |= indexed_points[vertex].X()!=points[vertex].X() ||
              indexed_points[vertex].Y()!=points[vertex].Y() ||
              indexed_points[vertex].Z()!=points[vertex].Z();
        } else {
          output.positions.push_back(static_cast<float>(points[vertex].X()));
          output.positions.push_back(static_cast<float>(points[vertex].Y()));
          output.positions.push_back(static_cast<float>(points[vertex].Z()));
          append_vec(output.normals, gp_Vec(normal));
          output.indices.push_back(static_cast<std::uint32_t>(output.indices.size()));
        }
      }
      if (export_index) {
        const gp_Vec actual_normal=gp_Vec(indexed_points[0],indexed_points[1]).Crossed(
            gp_Vec(indexed_points[0],indexed_points[2]));
        const double normal2=actual_normal.SquareMagnitude();
        const auto reject=[&](const std::string& why) {
          std::ostringstream diagnostic; diagnostic.precision(12);
          diagnostic << "Native export topology face " << face_index-1 << " triangle " << triangle_index <<
              " nodes " << indices[0] << '/' << indices[1] << '/' << indices[2] << ": " << why << "; old/new XYZ";
          for (int i=0;i<3;++i) diagnostic << " (" << points[i].X() << ',' << points[i].Y() << ',' << points[i].Z() <<
              ")->(" << indexed_points[i].X() << ',' << indexed_points[i].Y() << ',' << indexed_points[i].Z() << ')';
          throw std::runtime_error(diagnostic.str());
        };
        if (!std::isfinite(normal2) || normal2<=0.0 || actual_normal.Dot(triangle_normal)<=0.0)
          reject("native facet collapsed or reversed after topology indexing");
        if (indexed_changed) {
          const double normal_angle=actual_normal.Angle(triangle_normal);
          if (!std::isfinite(normal_angle) || normal_angle>angular)
            reject("native indexing exceeds requested facet angular precision: "+std::to_string(normal_angle));
          if (!triangulation->HasUVNodes()) reject("changed shared boundary has no source UV precision witness");
          const gp_Pnt2d uv[3]={triangulation->UVNode(indices[0]),triangulation->UVNode(indices[1]),
              triangulation->UVNode(indices[2])};
          // Native estimator samples, not an all-point Hausdorff claim. The
          // source BRep and triangulation remain untouched by export indexing.
          const double weights[7][3]={{1,0,0},{0,1,0},{0,0,1},{.5,.5,0},{0,.5,.5},{.5,0,.5},
              {1.0/3,1.0/3,1.0/3}};
          for (const auto& w : weights) {
            const double u=w[0]*uv[0].X()+w[1]*uv[1].X()+w[2]*uv[2].X();
            const double v=w[0]*uv[0].Y()+w[1]*uv[1].Y()+w[2]*uv[2].Y();
            const gp_Pnt affine(w[0]*indexed_points[0].X()+w[1]*indexed_points[1].X()+w[2]*indexed_points[2].X(),
                w[0]*indexed_points[0].Y()+w[1]*indexed_points[1].Y()+w[2]*indexed_points[2].Y(),
                w[0]*indexed_points[0].Z()+w[1]*indexed_points[1].Z()+w[2]*indexed_points[2].Z());
            if (!std::isfinite(u) || !std::isfinite(v)) reject("nonfinite source UV witness");
            gp_Pnt source_point; gp_Vec du,dv;
            surface.D1(u,v,source_point,du,dv);
            gp_Vec source_normal=du.Crossed(dv);
            if (face.Orientation()==TopAbs_REVERSED) source_normal.Reverse();
            const double source_normal2=source_normal.SquareMagnitude();
            if (!std::isfinite(source_normal2) || source_normal2<=0.0)
              reject("changed native boundary has no nonsingular source normal witness");
            const double source_angle=actual_normal.Angle(source_normal);
            if (!std::isfinite(source_angle) || source_angle>angular)
              reject("native indexed source sample exceeds requested angle: "+std::to_string(source_angle));
            const double gap=source_point.Distance(affine);
            if (!std::isfinite(gap) || gap>linear)
              reject("native indexed source sample exceeds requested deflection: "+std::to_string(gap));
          }
        }
      }
    }
    output.face_index_counts.push_back(
        static_cast<std::uint32_t>(output.indices.size()) -
        output.face_first_indices.back());
    if (output.face_index_counts.back() == 0) {
      GProp_GProps properties;
      BRepGProp::SurfaceProperties(face, properties);
      if (!std::isfinite(properties.Mass()) || std::abs(properties.Mass()) > 1e-14) {
        const std::string failure = "OCCT triangulation has no usable triangles for body " +
            std::to_string(body_id) + " face " + std::to_string(face_index - 1);
        if (!imported_display) throw std::runtime_error(failure);
        output.display_warning_face_indices.push_back(static_cast<std::uint32_t>(face_index - 1));
        output.display_warning_messages.push_back(rust::String(
            "Imported STEP face has no usable display triangles; exact geometry is retained. " + failure));
      }
    }
  }

  if (export_index) export_index->validate(output);
  output.edge_point_offsets.push_back(0);
  if (imported_display && output.indices.empty())
    throw std::runtime_error("Imported STEP has no valid display triangles; exact geometry cannot be displayed");
  TopTools_IndexedDataMapOfShapeListOfShape edge_faces;
  TopExp::MapShapesAndUniqueAncestors(shape, TopAbs_EDGE, TopAbs_FACE,
                                      edge_faces, false);
  for (int edge_index = 1; edge_index <= edge_map.Extent(); ++edge_index) {
    if (budget) budget->progress->check("edge extraction");
    const TopoDS_Edge edge = TopoDS::Edge(edge_map.FindKey(edge_index));
    bool refinable = false;
    if (edge_faces.Contains(edge)) {
      const TopTools_ListOfShape& adjacent_faces =
          edge_faces.FindFromKey(edge);
      if (adjacent_faces.Extent() == 2) {
        TopTools_ListIteratorOfListOfShape iterator(adjacent_faces);
        const TopoDS_Face first_face = TopoDS::Face(iterator.Value());
        iterator.Next();
        const TopoDS_Face second_face = TopoDS::Face(iterator.Value());
        refinable =
            BRep_Tool::Continuity(edge, first_face, second_face) == GeomAbs_C0;
      }
    }
    output.edge_refinable.push_back(refinable ? 1 : 0);
    BRepAdaptor_Curve curve(edge);
    append_circle(output.edge_circle_data, edge, curve);
    if (budget) {
      const auto points = sample_section_edge(edge, 0.01,
          budget->edge_points - output.edge_points.size() / 3, *budget->progress);
      for (const auto& point : points) append_point(output.edge_points, point);
    } else if (curve.GetType() == GeomAbs_Line) {
      append_point(output.edge_points, curve.Value(curve.FirstParameter()));
      append_point(output.edge_points, curve.Value(curve.LastParameter()));
    } else {


      GCPnts_UniformDeflection discretization(curve, 0.01, true);
      if (discretization.IsDone() && discretization.NbPoints() >= 2) {
        for (int point_index = 1;
             point_index <= discretization.NbPoints(); ++point_index) {
          append_point(output.edge_points, discretization.Value(point_index));
        }
      } else {
        const double first = curve.FirstParameter();
        const double last = curve.LastParameter();
        constexpr int sample_count = 25;
        for (int sample = 0; sample < sample_count; ++sample) {
          const double t =
              first + (last - first) * static_cast<double>(sample) /
                          static_cast<double>(sample_count - 1);
          append_point(output.edge_points, curve.Value(t));
        }
      }
    }
    output.edge_point_offsets.push_back(
        static_cast<std::uint32_t>(output.edge_points.size() / 3));
  }
  return output;
}

FfiInterferenceResult Kernel::exact_interference(
    const FfiBodyPlacement& placement_a,
    const FfiBodyPlacement& placement_b) const {
  const auto found_a = impl_->bodies.find(placement_a.body_id);
  const auto found_b = impl_->bodies.find(placement_b.body_id);
  if (found_a == impl_->bodies.end() || found_b == impl_->bodies.end()) {
    throw std::runtime_error("interference query references a missing body");
  }
  const std::array<double, 14> values = {
      placement_a.translation[0], placement_a.translation[1],
      placement_a.translation[2], placement_a.rotation[0],
      placement_a.rotation[1],    placement_a.rotation[2],
      placement_a.rotation[3],    placement_b.translation[0],
      placement_b.translation[1], placement_b.translation[2],
      placement_b.rotation[0],    placement_b.rotation[1],
      placement_b.rotation[2],    placement_b.rotation[3]};
  if (std::any_of(values.begin(), values.end(),
                  [](double value) { return !std::isfinite(value); })) {
    throw std::runtime_error("interference query transform is not finite");
  }
  auto placed = [](const TopoDS_Shape& shape, double tx, double ty, double tz,
                   double qx, double qy, double qz, double qw) {
    const double magnitude = std::sqrt(qx * qx + qy * qy + qz * qz + qw * qw);
    if (magnitude <= 1.0e-12) {
      throw std::runtime_error("interference query quaternion is degenerate");
    }
    gp_Trsf transform;
    transform.SetRotation(gp_Quaternion(qx / magnitude, qy / magnitude,
                                        qz / magnitude, qw / magnitude));
    transform.SetTranslationPart(gp_Vec(tx, ty, tz));
    return BRepBuilderAPI_Transform(shape, transform, true).Shape();
  };
  const TopoDS_Shape a =
      placed(found_a->second, placement_a.translation[0],
             placement_a.translation[1], placement_a.translation[2],
             placement_a.rotation[0], placement_a.rotation[1],
             placement_a.rotation[2], placement_a.rotation[3]);
  const TopoDS_Shape b =
      placed(found_b->second, placement_b.translation[0],
             placement_b.translation[1], placement_b.translation[2],
             placement_b.rotation[0], placement_b.rotation[1],
             placement_b.rotation[2], placement_b.rotation[3]);

  BRepExtrema_DistShapeShape distance(a, b);
  if (!distance.IsDone()) {
    throw std::runtime_error("OCCT could not evaluate exact body clearance");
  }
  FfiInterferenceResult output{};
  output.minimum_clearance_mm = distance.Value();
  if (distance.NbSolution() > 0) {
    const gp_Pnt point_a = distance.PointOnShape1(1);
    const gp_Pnt point_b = distance.PointOnShape2(1);
    output.closest_point_a_x = point_a.X();
    output.closest_point_a_y = point_a.Y();
    output.closest_point_a_z = point_a.Z();
    output.closest_point_b_x = point_b.X();
    output.closest_point_b_y = point_b.Y();
    output.closest_point_b_z = point_b.Z();
  }

  if (!distance.InnerSolution() && output.minimum_clearance_mm > 1.0e-7) {
    return output;
  }
  BRepAlgoAPI_Common common(a, b, Message_ProgressRange());
  if (!common.IsDone()) {
    throw std::runtime_error("OCCT could not evaluate exact body overlap");
  }
  const TopoDS_Shape overlap = common.Shape();
  if (!overlap.IsNull()) {
    GProp_GProps properties;
    BRepGProp::VolumeProperties(overlap, properties);
    output.overlap_volume_mm3 = std::abs(properties.Mass());
  }
  return output;
}

FfiDrawingProjection Kernel::drawing_projection(
    const rust::Vec<std::uint64_t>& requested_body_ids,
    const rust::Vec<FfiBodyPlacement>& occurrences,
    const FfiDrawingOptions& options) const {
  if (impl_->bodies.empty()) {
    throw std::runtime_error("there are no active bodies to project");
  }
  gp_Vec direction(options.direction[0], options.direction[1],
                   options.direction[2]);
  gp_Vec up(options.up[0], options.up[1], options.up[2]);
  if (direction.SquareMagnitude() < 1.0e-18 || up.SquareMagnitude() < 1.0e-18) {
    throw std::runtime_error("drawing projection basis is degenerate");
  }
  direction.Normalize();

  gp_Vec right = up.Crossed(direction);
  if (right.SquareMagnitude() < 1.0e-18) {
    throw std::runtime_error(
        "drawing projection direction and up are parallel");
  }
  right.Normalize();

  std::vector<TopoDS_Shape> source_shapes;
  if (options.assembly_scope) {
    if (occurrences.empty()) {
      throw std::runtime_error("assembly drawing contains no occurrences");
    }
    for (const auto& occurrence : occurrences) {
      const auto found = impl_->bodies.find(occurrence.body_id);
      if (found == impl_->bodies.end()) {
        throw std::runtime_error(
            "drawing occurrence references a missing body");
      }
      const auto& t = occurrence.translation;
      const auto& q = occurrence.rotation;
      const double magnitude =
          std::sqrt(q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]);
      if (!std::isfinite(magnitude) || magnitude <= 1.0e-12 ||
          !std::isfinite(t[0]) || !std::isfinite(t[1]) ||
          !std::isfinite(t[2])) {
        throw std::runtime_error(
            "drawing occurrence placement is not a finite rigid transform");
      }
      gp_Trsf transform;
      transform.SetRotation(gp_Quaternion(q[0] / magnitude, q[1] / magnitude,
                                          q[2] / magnitude, q[3] / magnitude));
      transform.SetTranslationPart(gp_Vec(t[0], t[1], t[2]));
      source_shapes.push_back(
          BRepBuilderAPI_Transform(found->second, transform, true).Shape());
    }
  } else if (requested_body_ids.empty()) {
    for (const auto& [body_id, shape] : impl_->bodies) {
      (void)body_id;
      source_shapes.push_back(shape);
    }
  } else {
    std::set<std::uint64_t> unique_ids;
    for (const std::uint64_t body_id : requested_body_ids) {
      if (!unique_ids.insert(body_id).second) {
        continue;
      }
      const auto found = impl_->bodies.find(body_id);
      if (found == impl_->bodies.end()) {
        throw std::runtime_error("selected drawing body is missing");
      }
      source_shapes.push_back(found->second);
    }
  }

  gp_Vec section_normal(options.section_normal[0], options.section_normal[1],
                        options.section_normal[2]);
  const gp_Pnt section_point(options.section_point[0], options.section_point[1],
                             options.section_point[2]);
  if (options.has_section_plane) {
    if (section_normal.SquareMagnitude() < 1.0e-18) {
      throw std::runtime_error("drawing section plane normal is degenerate");
    }
    section_normal.Normalize();
    if (options.has_section_depth && (!std::isfinite(options.section_depth) ||
                                      options.section_depth <= 0.0)) {
      throw std::runtime_error("drawing section depth must be positive");
    }
  }

  std::vector<TopoDS_Shape> projection_shapes;
  projection_shapes.reserve(source_shapes.size());
  for (const TopoDS_Shape& source : source_shapes) {
    if (!options.has_section_plane) {
      projection_shapes.push_back(source);
      continue;
    }
    const gp_Pln front_plane(section_point, gp_Dir(section_normal));
    const gp_Pnt behind_front =
        section_point.Translated(section_normal.Multiplied(-1.0));
    TopoDS_Shape clipped = retain_half_space(source, front_plane, behind_front);
    if (options.has_section_depth && !clipped.IsNull()) {
      const gp_Pnt back_point = section_point.Translated(
          section_normal.Multiplied(-options.section_depth));
      const gp_Pln back_plane(back_point, gp_Dir(section_normal));
      const gp_Pnt inside_slab = section_point.Translated(
          section_normal.Multiplied(-options.section_depth * 0.5));
      clipped = retain_half_space(clipped, back_plane, inside_slab);
    }
    if (!clipped.IsNull()) {
      projection_shapes.push_back(clipped);
    }
  }

  Handle(HLRBRep_Algo) algorithm = new HLRBRep_Algo();
  for (const TopoDS_Shape& shape : projection_shapes) {
    algorithm->Add(shape);
  }
  algorithm->Projector(HLRAlgo_Projector(
      gp_Ax2(gp_Pnt(0.0, 0.0, 0.0), gp_Dir(direction), gp_Dir(right))));
  algorithm->Update();
  algorithm->Hide();

  HLRBRep_HLRToShape extractor(algorithm);
  FfiDrawingProjection output;
  output.visible_offsets.push_back(0);
  output.hidden_offsets.push_back(0);
  output.section_offsets.push_back(0);
  std::set<std::vector<std::int64_t>> seen;
  const double curve_deflection = std::max(1.0e-4, options.deflection);
  append_projection_shape(extractor.VCompound(), curve_deflection,
                          output.visible_offsets, output.visible_points, seen);
  append_projection_shape(extractor.OutLineVCompound(), curve_deflection,
                          output.visible_offsets, output.visible_points, seen);
  if (options.include_tangent_edges) {
    append_projection_shape(extractor.Rg1LineVCompound(), curve_deflection,
                            output.visible_offsets, output.visible_points,
                            seen);
    append_projection_shape(extractor.RgNLineVCompound(), curve_deflection,
                            output.visible_offsets, output.visible_points,
                            seen);
  }
  if (options.include_hidden) {
    append_projection_shape(extractor.HCompound(), curve_deflection,
                            output.hidden_offsets, output.hidden_points, seen);
    append_projection_shape(extractor.OutLineHCompound(), curve_deflection,
                            output.hidden_offsets, output.hidden_points, seen);
    if (options.include_tangent_edges) {
      append_projection_shape(extractor.Rg1LineHCompound(), curve_deflection,
                              output.hidden_offsets, output.hidden_points,
                              seen);
      append_projection_shape(extractor.RgNLineHCompound(), curve_deflection,
                              output.hidden_offsets, output.hidden_points,
                              seen);
    }
  }
  if (options.has_section_plane) {
    const gp_Pln cutting_plane(section_point, gp_Dir(section_normal));
    gp_Vec page_up = direction.Crossed(right);
    page_up.Normalize();
    std::set<std::vector<std::int64_t>> section_seen;
    Handle(SectionProgress) progress = new SectionProgress(30'000);
    SectionRegionBudget region_budget{100'000, 16 * 1024 * 1024, progress.get()};
    Message_ProgressScope regions(progress->Start(), "Drawing material sections", source_shapes.size());
    for (const TopoDS_Shape& shape : source_shapes) {
      Message_ProgressScope member(regions.Next(), "Drawing section body", 2);
      section_shape_complexity(shape, region_budget, "drawing section source topology");
      const auto boundaries = material_section_boundary(shape, cutting_plane,
                                                        region_budget, member.Next());
      if (boundaries.IsNull()) {
        // Contact outlines remain visible geometry, never material hatching.
        // A fresh child range still belongs to the original drawing deadline.
        const auto complexity = section_shape_complexity(shape, region_budget,
                                                          "drawing boundary input");
        region_budget.boolean_work(complexity, 1, "drawing boundary intersection");
        const auto outline = exact_section_shape(shape, cutting_plane, member.Next());
        progress->check("drawing boundary intersection");
        append_section_shape(outline, right, page_up, curve_deflection,
                             output.visible_offsets, output.visible_points,
                             section_seen, progress.get(), 100'000);
        continue;
      }
      append_section_shape(boundaries, right, page_up,
                           curve_deflection, output.section_offsets,
                           output.section_points, section_seen, progress.get(), 100'000);
    }
  }
  return output;
}

rust::Vec<std::uint8_t> Kernel::export_step(
    const rust::Vec<std::uint64_t>& requested_body_ids,
    rust::Str thread_metadata_hex,
    rust::Str occurrence_placements_hex) const {
  if (impl_->bodies.empty()) {
    throw std::runtime_error("there are no active bodies to export");
  }
  STEPControl_Writer writer;
  if (!Interface_Static::SetIVal("write.step.schema", 5)) {
    throw std::runtime_error("OCCT does not expose the AP242 STEP schema");
  }



  (void)writer.Model(Standard_True);
  auto transfer = [&](const TopoDS_Shape& shape) {
    const IFSelect_ReturnStatus status =
        writer.Transfer(shape, STEPControl_AsIs, true, Message_ProgressRange());
    if (status != IFSelect_RetDone) {
      throw std::runtime_error("OCCT could not transfer a body to STEP");
    }
  };
  constexpr std::size_t kOccurrenceRecordBytes = 3 * sizeof(std::uint64_t) +
                                                  7 * sizeof(double);
  auto decode_hex = [](rust::Str input) {
    if (input.size() % 2 != 0) {
      throw std::runtime_error("STEP occurrence placement payload has odd hex length");
    }
    auto nibble = [](char value) -> std::uint8_t {
      if (value >= '0' && value <= '9') return value - '0';
      if (value >= 'a' && value <= 'f') return value - 'a' + 10;
      if (value >= 'A' && value <= 'F') return value - 'A' + 10;
      throw std::runtime_error("STEP occurrence placement payload is not hexadecimal");
    };
    std::vector<std::uint8_t> bytes;
    bytes.reserve(input.size() / 2);
    const char* data = input.data();
    for (std::size_t index = 0; index < input.size(); index += 2) {
      bytes.push_back(static_cast<std::uint8_t>(
          (nibble(data[index]) << 4) | nibble(data[index + 1])));
    }
    return bytes;
  };
  const auto placement_bytes = decode_hex(occurrence_placements_hex);
  if (placement_bytes.size() % kOccurrenceRecordBytes != 0) {
    throw std::runtime_error("STEP occurrence placement payload is truncated");
  }
  auto read_u64 = [&](std::size_t offset) {
    std::uint64_t value = 0;
    for (std::size_t index = 0; index < sizeof(value); ++index) {
      value |= static_cast<std::uint64_t>(placement_bytes[offset + index]) << (index * 8);
    }
    return value;
  };
  auto read_f64 = [&](std::size_t offset) {
    const std::uint64_t bits = read_u64(offset);
    double value = 0.0;
    static_assert(sizeof(value) == sizeof(bits));
    std::memcpy(&value, &bits, sizeof(value));
    return value;
  };

  if (!placement_bytes.empty()) {
    for (std::size_t offset = 0; offset < placement_bytes.size();
         offset += kOccurrenceRecordBytes) {
      const std::uint64_t body_id = read_u64(offset);



      (void)read_u64(offset + 8);
      (void)read_u64(offset + 16);
      const double tx = read_f64(offset + 24);
      const double ty = read_f64(offset + 32);
      const double tz = read_f64(offset + 40);
      const double qx = read_f64(offset + 48);
      const double qy = read_f64(offset + 56);
      const double qz = read_f64(offset + 64);
      const double qw = read_f64(offset + 72);
      const auto found = impl_->bodies.find(body_id);
      if (found == impl_->bodies.end()) {
        throw std::runtime_error("assembly STEP occurrence references a missing body");
      }
      const double magnitude = std::sqrt(qx * qx + qy * qy + qz * qz + qw * qw);
      if (magnitude <= 1.0e-12 || !std::isfinite(magnitude)) {
        throw std::runtime_error("assembly STEP occurrence rotation is degenerate");
      }
      gp_Trsf transform;
      transform.SetRotation(gp_Quaternion(
          qx / magnitude, qy / magnitude, qz / magnitude, qw / magnitude));
      transform.SetTranslationPart(gp_Vec(tx, ty, tz));
      transfer(BRepBuilderAPI_Transform(found->second, transform, true).Shape());
    }
  } else if (requested_body_ids.empty()) {
    for (const auto& [body_id, shape] : impl_->bodies) {
      (void)body_id;
      transfer(shape);
    }
  } else {
    for (const std::uint64_t body_id : requested_body_ids) {
      const auto found = impl_->bodies.find(body_id);
      if (found == impl_->bodies.end()) {
        throw std::runtime_error("selected STEP export body is missing");
      }
      transfer(found->second);
    }
  }

  if (thread_metadata_hex.size() > 4) {
    const std::string metadata(thread_metadata_hex.data(),
                               thread_metadata_hex.size());
    const std::string description =
        "Limo CAD AP242; LIMO_CAD_THREAD_METADATA_V1_HEX=" + metadata;
    const Handle(StepData_StepModel) model = writer.Model(Standard_False);
    APIHeaderSection_MakeHeader header(model);
    Handle(Interface_HArray1OfHAsciiString) descriptions =
        new Interface_HArray1OfHAsciiString(1, 1);
    descriptions->SetValue(
        1, new TCollection_HAsciiString(description.c_str()));
    header.SetDescription(descriptions);
    header.Apply(model);
  }

  std::ostringstream stream;
  if (writer.WriteStream(stream) != IFSelect_RetDone) {
    throw std::runtime_error("OCCT could not write the STEP stream");
  }
  const std::string bytes = stream.str();
  rust::Vec<std::uint8_t> output;
  output.reserve(bytes.size());
  for (const unsigned char byte : bytes) {
    output.push_back(byte);
  }
  return output;
}

std::unique_ptr<Kernel> new_kernel() {



  static const bool globals_initialized = [] {
    auto messenger = Message::DefaultMessenger();
    messenger->RemovePrinters(STANDARD_TYPE(Message_PrinterOStream));
    Handle(Message_PrinterOStream) printer =
        new Message_PrinterOStream("cerr", Standard_True);
    printer->SetToColorize(Standard_False);
    messenger->AddPrinter(printer);






    (void)BRepLib::Plane();
    return true;
  }();
  (void)globals_initialized;
  return std::make_unique<Kernel>();
}

}
