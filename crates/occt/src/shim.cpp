#include "nbcad-occt/src/native.rs.h"

#include <APIHeaderSection_MakeHeader.hxx>
#include <BRepAdaptor_Curve.hxx>
#include <BRepAdaptor_Surface.hxx>
#include <BRepAlgoAPI_Common.hxx>
#include <BRepAlgoAPI_Cut.hxx>
#include <BRepAlgoAPI_Fuse.hxx>
#include <BRepAlgoAPI_Section.hxx>
#include <BRepAlgoAPI_Splitter.hxx>
#include <BRepBndLib.hxx>
#include <BRepClass3d_SolidClassifier.hxx>
#include <BRepBuilderAPI_MakeEdge.hxx>
#include <BRepBuilderAPI_MakeFace.hxx>
#include <BRepBuilderAPI_MakePolygon.hxx>
#include <BRepBuilderAPI_MakeWire.hxx>
#include <BRepBuilderAPI_Sewing.hxx>
#include <BRepBuilderAPI_Transform.hxx>
#include <BRepBuilderAPI_TransitionMode.hxx>
#include <BRepCheck_Analyzer.hxx>
#include <BRepExtrema_DistShapeShape.hxx>
#include <BRepFill.hxx>
#include <BRepMesh_IncrementalMesh.hxx>
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
#include <BRep_Tool.hxx>
#include <BRep_Builder.hxx>
#include <Bnd_Box.hxx>
#include <GeomAbs_CurveType.hxx>
#include <GeomAbs_Shape.hxx>
#include <GeomAbs_SurfaceType.hxx>
#include <Geom2d_Line.hxx>
#include <Geom_CylindricalSurface.hxx>
#include <GCPnts_UniformDeflection.hxx>
#include <GC_MakeArcOfCircle.hxx>
#include <GProp_GProps.hxx>
#include <HLRAlgo_Projector.hxx>
#include <HLRBRep_Algo.hxx>
#include <HLRBRep_HLRToShape.hxx>
#include <Message_ProgressRange.hxx>
#include <Message.hxx>
#include <Message_Messenger.hxx>
#include <Message_PrinterOStream.hxx>
#include <IFSelect_ReturnStatus.hxx>
#include <Interface_Static.hxx>
#include <Interface_HArray1OfHAsciiString.hxx>
#include <Poly_Triangulation.hxx>
#include <STEPControl_StepModelType.hxx>
#include <STEPControl_Reader.hxx>
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
#include <gp_Dir.hxx>
#include <gp_Dir2d.hxx>
#include <gp_Pln.hxx>
#include <gp_Pnt.hxx>
#include <gp_Pnt2d.hxx>
#include <gp_Quaternion.hxx>
#include <gp_Trsf.hxx>
#include <gp_Vec.hxx>

#include <algorithm>
#include <array>
#include <cmath>
#include <cstdint>
#include <cstring>
#include <map>
#include <set>
#include <sstream>
#include <stdexcept>
#include <string>
#include <utility>
#include <vector>

namespace nbcad_occt {
namespace {

constexpr double kPi = 3.14159265358979323846;
constexpr double kTau = kPi * 2.0;

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
    const Handle(Geom_CylindricalSurface)& surface) {
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
  BRepLib::BuildCurve3d(edge);
  BRepLib::SameParameter(edge, 1e-7);
  return edge;
}

TopoDS_Shape make_continuous_thread_cutter(
    const gp_Ax2& axis,
    double spine_radius,
    const std::vector<std::pair<double, double>>& radius_half_widths,
    double pitch,
    double thread_depth,
    bool left_hand,
    const char* label) {
  // Extend by a full pitch at both ends.  The Boolean target trims the sweep
  // to the requested thread depth, avoiding partial open grooves and cutter
  // caps at the model ends.
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

  // Build the thread volume from exact helical boundary rails.  A pipe sweep
  // is deliberately avoided here: its moving trihedron can rotate the ISO
  // section away from the requested axial widths and, for a closed helix,
  // can leave OCCT with the complementary solid.  These ruled flanks keep
  // every radius and axial profile station explicit and deterministic.
  BRepBuilderAPI_Sewing sewing(1e-7, true, true, true, false);
  const int segment_count = static_cast<int>(std::ceil(turns - 1e-10));
  for (int segment_index = 0; segment_index < segment_count;
       ++segment_index) {
    const double segment_start = center_start + segment_index * pitch;
    const double segment_end = std::min(segment_start + pitch, center_end);
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
  // A thread cutter must contain its whole radial profile while excluding the
  // thread axis. Check both boundaries explicitly so a future face-ordering
  // change cannot silently turn the cutter into its unbounded complement.
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

  // Compatibility fallback for plans created before analytic curve metadata.
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
    // Uniform scale about the face centroid followed by translation along
    // the source normal. Applying this to TopoDS wires preserves their exact
    // analytic edges rather than rebuilding them from tessellation.
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
    std::set<std::vector<std::int64_t>>& seen) {
  if (shape.IsNull()) {
    return;
  }
  constexpr double kQuantize = 1.0e7;
  for (TopExp_Explorer explorer(shape, TopAbs_EDGE); explorer.More(); explorer.Next()) {
    const TopoDS_Edge edge = TopoDS::Edge(explorer.Current());
    const std::vector<gp_Pnt> points = sample_projection_edge(edge, deflection);
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
  gp_Dir normal = axes.Direction();
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

  gp_Dir normal = surface.Plane().Position().Direction();
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

}  // namespace

class Kernel::Impl {
 public:
  std::map<std::uint64_t, TopoDS_Shape> bodies;
};

Kernel::Kernel() : impl_(std::make_unique<Impl>()) {}
Kernel::~Kernel() = default;

void Kernel::reset() { impl_->bodies.clear(); }

void Kernel::apply_job(const FfiJob& job) {
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
    if (job.kind == 5) {
      BRepFilletAPI_MakeFillet fillet(found->second);
      for (const std::uint32_t index : job.edge_indices) {
        if (index >= static_cast<std::uint32_t>(edge_map.Extent())) {
          throw std::runtime_error("referenced fillet edge no longer exists");
        }
        fillet.Add(job.radius, TopoDS::Edge(edge_map.FindKey(index + 1)));
      }
      fillet.Build(Message_ProgressRange());
      if (!fillet.IsDone()) {
        throw std::runtime_error("OCCT could not build the selected solid fillet");
      }
      found->second = fillet.Shape();
    } else {
      BRepFilletAPI_MakeChamfer chamfer(found->second);
      for (const std::uint32_t index : job.edge_indices) {
        if (index >= static_cast<std::uint32_t>(edge_map.Extent())) {
          throw std::runtime_error("referenced chamfer edge no longer exists");
        }
        chamfer.Add(job.radius, TopoDS::Edge(edge_map.FindKey(index + 1)));
      }
      chamfer.Build(Message_ProgressRange());
      if (!chamfer.IsDone()) {
        throw std::runtime_error("OCCT could not build the selected solid chamfer");
      }
      found->second = chamfer.Shape();
    }
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
    // A tap-drill diameter is process guidance, not a finished thread limit.
    // Modeled threads open the bore to the selected class's GO minor diameter.
    const double finished_hole_diameter =
        job.thread_mode == 2 ? job.thread_minor_diameter : job.diameter;
    BRepPrimAPI_MakeCylinder main_cylinder(axis, finished_hole_diameter * 0.5,
                                           hole_depth + overlap * 2.0);
    TopoDS_Shape cutter = main_cylinder.Shape();
    std::vector<TopoDS_Shape> thread_cutters;
    if (job.hole_style == 1) {
      BRepPrimAPI_MakeCylinder counterbore(
          axis, job.secondary_diameter * 0.5,
          job.secondary_depth + overlap * 2.0);
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
      BRepPrimAPI_MakeCone countersink(axis, large_radius, small_radius,
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
      // Use an axis rooted on the support face. The base cutter starts a
      // fraction outside only to keep booleans watertight.
      const gp_Ax2 thread_axis(support, gp_Dir(direction), axis.XDirection());
      thread_cutters = make_internal_thread_cutters(
          thread_axis, job.thread_major_diameter,
          job.thread_pitch_diameter, job.thread_minor_diameter,
          job.thread_pitch, requested_thread_depth, job.thread_left_hand);
    }
    if (!job.through_all && job.hole_bottom_style == 1) {
      const double half_angle = job.drill_point_angle_deg * kPi / 360.0;
      const double tip_depth =
          (finished_hole_diameter * 0.5) / std::tan(half_angle);
      if (!std::isfinite(tip_depth) || tip_depth <= 0.0) {
        throw std::runtime_error("drill point angle is invalid");
      }
      const gp_Pnt tip_start = support.Translated(
          direction.Multiplied(hole_depth - overlap));
      const gp_Ax2 tip_axis(tip_start, gp_Dir(direction));
      BRepPrimAPI_MakeCone drill_point(
          tip_axis, finished_hole_diameter * 0.5, 0.0,
          tip_depth + overlap);
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
    } else {
      // Subtract the helical tool before opening the predrill bore. Passing
      // both overlapping tools as a compound can preserve the removed thread
      // volume as a detached second solid, visually filling the groove.
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
        // ISO external classes carry a negative allowance. Remove the thin
        // nominal crest sleeve first so the final B-rep is the 6g GO envelope
        // instead of the basic-size source cylinder.
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
      const std::vector<TopoDS_Shape> cutters =
          make_external_thread_cutters(
              thread_axis, job.thread_major_diameter,
              job.thread_pitch_diameter, job.thread_minor_diameter,
              job.thread_pitch, requested_depth, job.thread_left_hand);
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
    found->second = shell.Shape();
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
      // Fuse all tools in one interference pass. Repeatedly fusing into the
      // growing result makes ordinary tooth/body patterns needlessly quadratic.
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
    if (!job.keep_tools) {
      for (std::size_t index = 1; index < job.target_body_ids.size(); ++index) {
        impl_->bodies.erase(job.target_body_ids[index]);
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

    // One general-fuse pass produces both sides and shares the expensive
    // intersection graph. The previous pair of Common operations calculated
    // the same plane/body intersection twice. Classify the resulting solids
    // by their exact volume centroids to retain the established positive-side
    // and negative-side output identities.
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
    // General fuse can emit a detached, near-zero-fill sliver where a swept
    // thread crosses the splitting plane. It spans a large bounding box but
    // has only a few millionths of the real side's volume. Drop only records
    // that are both tiny relative to their side and demonstrably sliver-like;
    // compact small solids and legitimate multi-region split outputs remain.
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
      // A general-fuse plane that intersects a helical face can occasionally
      // retain duplicated seam pcurves. Shape healing can discard an inner
      // wire in that case, so regenerate only this exceptional split from two
      // exact half-space Commons. Ordinary splits remain on the single-pass
      // fast path above.
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
    // `source_face_index` is only a legacy/debug hint. OCCT map ordering can
    // change after an upstream edit, so resolve the unique exact signature and
    // fail safely when the reference changed or became ambiguous.
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

FfiMesh Kernel::mesh(std::uint64_t body_id) const {
  return mesh_with_deflection(body_id, 0.15, 0.35);
}

FfiMesh Kernel::mesh_with_deflection(
    std::uint64_t body_id,
    double linear_deflection,
    double angular_deflection) const {
  const auto found = impl_->bodies.find(body_id);
  if (found == impl_->bodies.end()) {
    throw std::runtime_error("body is missing");
  }
  const TopoDS_Shape& shape = found->second;
  const double linear =
      linear_deflection > 0.0 ? linear_deflection : 0.15;
  const double angular =
      angular_deflection > 0.0 ? angular_deflection : 0.35;
  BRepMesh_IncrementalMesh mesher(shape, linear, false, angular, true);
  mesher.Perform();

  FfiMesh output;
  output.body_id = body_id;
  TopTools_IndexedMapOfShape face_map;
  TopExp::MapShapes(shape, TopAbs_FACE, face_map);
  for (int face_index = 1; face_index <= face_map.Extent(); ++face_index) {
    const TopoDS_Face face = TopoDS::Face(face_map.FindKey(face_index));
    TopLoc_Location location;
    const Handle(Poly_Triangulation) triangulation =
        BRep_Tool::Triangulation(face, location);
    if (triangulation.IsNull()) {
      continue;
    }
    if (!triangulation->HasNormals()) {
      triangulation->ComputeNormals();
    }
    output.face_first_indices.push_back(
        static_cast<std::uint32_t>(output.indices.size()));
    const gp_Trsf transform = location.Transformation();
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
      if (triangle_normal.SquareMagnitude() <= 1e-24) {
        continue;
      }
      for (int vertex = 0; vertex < 3; ++vertex) {
        gp_Dir normal = triangulation->Normal(indices[vertex]);
        normal.Transform(transform);
        if (face.Orientation() == TopAbs_REVERSED) {
          normal.Reverse();
        }
        output.positions.push_back(static_cast<float>(points[vertex].X()));
        output.positions.push_back(static_cast<float>(points[vertex].Y()));
        output.positions.push_back(static_cast<float>(points[vertex].Z()));
        append_vec(output.normals, gp_Vec(normal));
        output.indices.push_back(
            static_cast<std::uint32_t>(output.indices.size()));
      }
    }
    output.face_index_counts.push_back(
        static_cast<std::uint32_t>(output.indices.size()) -
        output.face_first_indices.back());
    append_plane(output.face_plane_data, face);
    append_face_signature(output.face_signature_data, face);
    append_cylinder(output.face_cylinder_data, face);
  }

  output.edge_point_offsets.push_back(0);
  TopTools_IndexedMapOfShape edge_map;
  TopExp::MapShapes(shape, TopAbs_EDGE, edge_map);
  TopTools_IndexedDataMapOfShapeListOfShape edge_faces;
  TopExp::MapShapesAndUniqueAncestors(shape, TopAbs_EDGE, TopAbs_FACE,
                                      edge_faces, false);
  for (int edge_index = 1; edge_index <= edge_map.Extent(); ++edge_index) {
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
    if (curve.GetType() == GeomAbs_Line) {
      append_point(output.edge_points, curve.Value(curve.FirstParameter()));
      append_point(output.edge_points, curve.Value(curve.LastParameter()));
    } else {
      // Helical thread edges need a tighter display chord than ordinary CAD
      // edges; a coarse chord visibly cuts across the minor-diameter envelope.
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
    std::uint64_t body_a,
    double translation_a_x,
    double translation_a_y,
    double translation_a_z,
    double rotation_a_x,
    double rotation_a_y,
    double rotation_a_z,
    double rotation_a_w,
    std::uint64_t body_b,
    double translation_b_x,
    double translation_b_y,
    double translation_b_z,
    double rotation_b_x,
    double rotation_b_y,
    double rotation_b_z,
    double rotation_b_w) const {
  const auto found_a = impl_->bodies.find(body_a);
  const auto found_b = impl_->bodies.find(body_b);
  if (found_a == impl_->bodies.end() || found_b == impl_->bodies.end()) {
    throw std::runtime_error("interference query references a missing body");
  }
  const std::array<double, 14> values = {
      translation_a_x, translation_a_y, translation_a_z,
      rotation_a_x, rotation_a_y, rotation_a_z, rotation_a_w,
      translation_b_x, translation_b_y, translation_b_z,
      rotation_b_x, rotation_b_y, rotation_b_z, rotation_b_w};
  if (std::any_of(values.begin(), values.end(),
                  [](double value) { return !std::isfinite(value); })) {
    throw std::runtime_error("interference query transform is not finite");
  }
  auto placed = [](const TopoDS_Shape& shape,
                   double tx, double ty, double tz,
                   double qx, double qy, double qz, double qw) {
    const double magnitude = std::sqrt(qx * qx + qy * qy + qz * qz + qw * qw);
    if (magnitude <= 1.0e-12) {
      throw std::runtime_error("interference query quaternion is degenerate");
    }
    gp_Trsf transform;
    transform.SetRotation(gp_Quaternion(
        qx / magnitude, qy / magnitude, qz / magnitude, qw / magnitude));
    transform.SetTranslationPart(gp_Vec(tx, ty, tz));
    return BRepBuilderAPI_Transform(shape, transform, true).Shape();
  };
  const TopoDS_Shape a = placed(found_a->second,
      translation_a_x, translation_a_y, translation_a_z,
      rotation_a_x, rotation_a_y, rotation_a_z, rotation_a_w);
  const TopoDS_Shape b = placed(found_b->second,
      translation_b_x, translation_b_y, translation_b_z,
      rotation_b_x, rotation_b_y, rotation_b_z, rotation_b_w);

  BRepExtrema_DistShapeShape distance(a, b);
  distance.Perform();
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

  BRepAlgoAPI_Common common(a, b, Message_ProgressRange());
  common.Build(Message_ProgressRange());
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
    const rust::Vec<FfiDrawingOccurrence>& occurrences,
    bool assembly_scope,
    double direction_x,
    double direction_y,
    double direction_z,
    double up_x,
    double up_y,
    double up_z,
    bool include_hidden,
    bool include_tangent_edges,
    double deflection,
    bool has_section_plane,
    double section_point_x,
    double section_point_y,
    double section_point_z,
    double section_normal_x,
    double section_normal_y,
    double section_normal_z,
    bool has_section_depth,
    double section_depth) const {
  if (impl_->bodies.empty()) {
    throw std::runtime_error("there are no active bodies to project");
  }
  gp_Vec direction(direction_x, direction_y, direction_z);
  gp_Vec up(up_x, up_y, up_z);
  if (direction.SquareMagnitude() < 1.0e-18 || up.SquareMagnitude() < 1.0e-18) {
    throw std::runtime_error("drawing projection basis is degenerate");
  }
  direction.Normalize();
  // gp_Ax2's third argument is page X. For a model-to-viewer direction and
  // page-up vector, up x direction gives page-right.
  gp_Vec right = up.Crossed(direction);
  if (right.SquareMagnitude() < 1.0e-18) {
    throw std::runtime_error("drawing projection direction and up are parallel");
  }
  right.Normalize();

  std::vector<TopoDS_Shape> source_shapes;
  if (assembly_scope) {
    if (occurrences.empty()) {
      throw std::runtime_error("assembly drawing contains no occurrences");
    }
    for (const auto& occurrence : occurrences) {
      const auto found = impl_->bodies.find(occurrence.body_id);
      if (found == impl_->bodies.end()) {
        throw std::runtime_error("drawing occurrence references a missing body");
      }
      const auto& t = occurrence.translation;
      const auto& q = occurrence.rotation;
      const double magnitude = std::sqrt(q[0]*q[0] + q[1]*q[1] + q[2]*q[2] + q[3]*q[3]);
      if (!std::isfinite(magnitude) || magnitude <= 1.0e-12 ||
          !std::isfinite(t[0]) || !std::isfinite(t[1]) || !std::isfinite(t[2])) {
        throw std::runtime_error("drawing occurrence placement is not a finite rigid transform");
      }
      gp_Trsf transform;
      transform.SetRotation(gp_Quaternion(q[0]/magnitude,q[1]/magnitude,q[2]/magnitude,q[3]/magnitude));
      transform.SetTranslationPart(gp_Vec(t[0],t[1],t[2]));
      source_shapes.push_back(BRepBuilderAPI_Transform(found->second, transform, true).Shape());
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

  gp_Vec section_normal(section_normal_x, section_normal_y, section_normal_z);
  const gp_Pnt section_point(section_point_x, section_point_y, section_point_z);
  if (has_section_plane) {
    if (section_normal.SquareMagnitude() < 1.0e-18) {
      throw std::runtime_error("drawing section plane normal is degenerate");
    }
    section_normal.Normalize();
    if (has_section_depth &&
        (!std::isfinite(section_depth) || section_depth <= 0.0)) {
      throw std::runtime_error("drawing section depth must be positive");
    }
  }

  auto retain_half_space = [](const TopoDS_Shape& source,
                              const gp_Pln& boundary,
                              const gp_Pnt& retained_point) {
    const TopoDS_Face face = BRepBuilderAPI_MakeFace(boundary).Face();
    const TopoDS_Solid half_space =
        BRepPrimAPI_MakeHalfSpace(face, retained_point).Solid();
    BRepAlgoAPI_Common common(source, half_space);
    common.Build();
    if (!common.IsDone()) {
      throw std::runtime_error("OCCT could not clip the drawing section");
    }
    return common.Shape();
  };

  std::vector<TopoDS_Shape> projection_shapes;
  projection_shapes.reserve(source_shapes.size());
  for (const TopoDS_Shape& source : source_shapes) {
    if (!has_section_plane) {
      projection_shapes.push_back(source);
      continue;
    }
    const gp_Pln front_plane(section_point, gp_Dir(section_normal));
    const gp_Pnt behind_front = section_point.Translated(section_normal.Multiplied(-1.0));
    TopoDS_Shape clipped = retain_half_space(source, front_plane, behind_front);
    if (has_section_depth && !clipped.IsNull()) {
      const gp_Pnt back_point =
          section_point.Translated(section_normal.Multiplied(-section_depth));
      const gp_Pln back_plane(back_point, gp_Dir(section_normal));
      const gp_Pnt inside_slab = section_point.Translated(
          section_normal.Multiplied(-section_depth * 0.5));
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
  const double curve_deflection = std::max(1.0e-4, deflection);
  append_projection_shape(extractor.VCompound(), curve_deflection,
                          output.visible_offsets, output.visible_points, seen);
  append_projection_shape(extractor.OutLineVCompound(), curve_deflection,
                          output.visible_offsets, output.visible_points, seen);
  if (include_tangent_edges) {
    append_projection_shape(extractor.Rg1LineVCompound(), curve_deflection,
                            output.visible_offsets, output.visible_points, seen);
    append_projection_shape(extractor.RgNLineVCompound(), curve_deflection,
                            output.visible_offsets, output.visible_points, seen);
  }
  if (include_hidden) {
    // Keep the same de-duplication set: a coincident visible curve wins over a
    // hidden result, avoiding double-stroked SVG output.
    append_projection_shape(extractor.HCompound(), curve_deflection,
                            output.hidden_offsets, output.hidden_points, seen);
    append_projection_shape(extractor.OutLineHCompound(), curve_deflection,
                            output.hidden_offsets, output.hidden_points, seen);
    if (include_tangent_edges) {
      append_projection_shape(extractor.Rg1LineHCompound(), curve_deflection,
                              output.hidden_offsets, output.hidden_points, seen);
      append_projection_shape(extractor.RgNLineHCompound(), curve_deflection,
                              output.hidden_offsets, output.hidden_points, seen);
    }
  }
  if (has_section_plane) {
    const gp_Pln cutting_plane(section_point, gp_Dir(section_normal));
    gp_Vec page_up = direction.Crossed(right);
    page_up.Normalize();
    std::set<std::vector<std::int64_t>> section_seen;
    for (const TopoDS_Shape& shape : source_shapes) {
      BRepAlgoAPI_Section section_operation(shape, cutting_plane, false);
      section_operation.Approximation(true);
      section_operation.Build();
      if (!section_operation.IsDone() || section_operation.Shape().IsNull()) {
        continue;
      }
      append_section_shape(
          section_operation.Shape(), right, page_up, curve_deflection,
          output.section_offsets, output.section_points, section_seen);
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
  // STEPControl_Writer constructs an AP214 model by default. OCCT requires
  // a fresh model after changing write.step.schema for that setting to take
  // effect.
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
      // Occurrence and component ids are retained in the transport record for
      // the AP242/XCAF hierarchy boundary; this first production slice emits
      // exact placed roots, which preserves geometry for CAM immediately.
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
  // `[]` is `5b5d` in hex.
  if (thread_metadata_hex.size() > 4) {
    const std::string metadata(thread_metadata_hex.data(),
                               thread_metadata_hex.size());
    const std::string description =
        "noBS CAD AP242; NBCAD_THREAD_METADATA_V1_HEX=" + metadata;
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
  // OCCT's default console printer uses stdout, which belongs to the MCP
  // JSON-RPC transport. Keep transfer diagnostics on stderr in all hosts.
  // Function-local static initialization runs once, even with multiple kernels.
  static const bool diagnostics_configured = [] {
    auto messenger = Message::DefaultMessenger();
    messenger->RemovePrinters(STANDARD_TYPE(Message_PrinterOStream));
    Handle(Message_PrinterOStream) printer =
        new Message_PrinterOStream("cerr", Standard_True);
    printer->SetToColorize(Standard_False);
    messenger->AddPrinter(printer);
    return true;
  }();
  (void)diagnostics_configured;
  return std::make_unique<Kernel>();
}

}  // namespace nbcad_occt
