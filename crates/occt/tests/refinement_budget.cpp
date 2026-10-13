#include "refinement_budget.hpp"
#include <cassert>
#include <limits>

using limo_cad_occt::RefinementBudget;
using limo_cad_occt::RefinementBudgetExceeded;

template <typename Action>
void rejects(Action action, const char* resource) {
  try {
    action();
    throw std::logic_error("Expected refinement resource rejection");
  } catch (const RefinementBudgetExceeded& error) {
    const std::string message = error.what();
    assert(message.find(resource) != std::string::npos);
    assert(message.find("face 7") != std::string::npos);
  }
}

int main() {
  RefinementBudget budget;
  budget.context = "before standard healing, face 7";
  budget.comparisons = 12;
  budget.samples = 10;
  budget.insertions = 2;
  budget.compare(2, 3);
  budget.sample(2, 3);
  budget.insert(2);
  // The same operation's second repair must not receive a fresh allowance.
  budget.context = "after standard healing, face 7";
  rejects([&] { budget.compare(2, 4); }, "segment comparison");
  assert(budget.comparisons == 6);
  budget.compare(2, 3);
  rejects([&] { budget.compare(1); }, "segment comparison");
  rejects([&] { budget.sample(3, 2); }, "sample allocation");
  assert(budget.samples == 4);
  budget.sample(2, 2);
  rejects([&] { budget.sample(); }, "sample allocation");
  rejects([&] { budget.insert(4096); }, "4096 points per edge");
  assert(budget.insertions == 1);
  budget.insert(4095);
  rejects([&] { budget.insert(2); }, "total insertion");
  // Hostile products cannot wrap into an apparently affordable allowance.
  budget.comparisons = budget.samples = std::numeric_limits<std::size_t>::max();
  rejects([&] { budget.compare(budget.comparisons, 2); }, "segment comparison");
  rejects([&] { budget.sample(budget.samples, 2); }, "sample allocation");
  budget.compare(budget.comparisons, 0);
  budget.sample(budget.samples, 0);
  assert(budget.samples == std::numeric_limits<std::size_t>::max());

  // Circular scanning and later junction recovery share the same allowance;
  // neither the recovery attempt nor the post-healing call may reset it.
  RefinementBudget junction;
  junction.context = "before standard healing, face 7";
  junction.comparisons = 20;
  junction.samples = 40;
  junction.compare(2, 4);
  junction.context = "before standard healing, junction repair, face 7";
  junction.compare(3, 3);
  junction.sample(4, 3); // two edge snapshots and possible native restoration
  junction.sample(4, 4); // three pcurve snapshots and possible native restoration
  assert(junction.comparisons == 3 && junction.samples == 12);
  junction.context = "after standard healing, junction repair, face 7";
  rejects([&] { junction.compare(2, 2); }, "segment comparison");
  rejects([&] { junction.sample(4, 4); }, "sample allocation");
  assert(junction.comparisons == 3 && junction.samples == 12);

  // Recovery can distinguish resource exhaustion through a base exception
  // reference; an ordinary geometry runtime_error must remain recoverable.
  const std::runtime_error geometry_error("invalid geometric repair");
  const std::exception& geometry_base = geometry_error;
  assert(dynamic_cast<const RefinementBudgetExceeded*>(&geometry_base) == nullptr);
  bool classified = false;
  try {
    junction.sample(13);
  } catch (const std::exception& error) {
    const auto* exhaustion = dynamic_cast<const RefinementBudgetExceeded*>(&error);
    assert(exhaustion != nullptr);
    assert(std::string(exhaustion->what()).find(junction.context) != std::string::npos);
    assert(std::string(exhaustion->what()).find("Simplify or split") != std::string::npos);
    classified = true;
  }
  assert(classified);
}
