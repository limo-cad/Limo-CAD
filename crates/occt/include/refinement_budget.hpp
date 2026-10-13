#pragma once

#include <cstddef>
#include <stdexcept>
#include <string>

namespace limo_cad_occt {
// Resource exhaustion is fatal for this meshing operation. Geometry recovery
// may catch ordinary runtime errors, but must not swallow this diagnostic.
struct RefinementBudgetExceeded final : std::runtime_error {
  using std::runtime_error::runtime_error;
};

// One instance belongs to an entire HealModel operation, including both
// circular repair calls. Charge work before loops and storage before growth.
struct RefinementBudget {
  std::size_t comparisons = 16 * 1024 * 1024;
  std::size_t samples = 1024 * 1024;
  std::size_t insertions = 65536;
  std::string context;

  [[noreturn]] void exhausted(const char* resource) const {
    throw RefinementBudgetExceeded(std::string("OCCT circular-boundary refinement exhausted ") +
        resource + " budget (" + context +
        "). Simplify or split the source boundary before retrying.");
  }
  void charge(std::size_t& remaining, std::size_t count, const char* resource) const {
    if (count > remaining) exhausted(resource);
    remaining -= count;
  }
  void compare(std::size_t a, std::size_t b = 1) {
    // Check without multiplying potentially hostile counts first.
    if (b != 0 && a > comparisons / b) exhausted("segment comparison");
    comparisons -= a * b;
  }
  void sample(std::size_t a = 1, std::size_t b = 1) {
    if (b != 0 && a > samples / b) exhausted("sample allocation");
    samples -= a * b;
  }
  void insert(std::size_t existing) {
    if (existing >= 4096) exhausted("4096 points per edge");
    charge(insertions, 1, "total insertion");
  }
};
} // namespace limo_cad_occt
