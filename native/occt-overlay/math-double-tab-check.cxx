#include <math_DoubleTab.hxx>
#include <Standard_Failure.hxx>
#include <limits>
#include <stdexcept>
#include <iostream>
#include <utility>

static void require(bool value)
{
  if (!value)
    throw std::runtime_error("checked OCCT matrix storage invariant failed");
}

template<typename Operation> static void rejected(Operation operation)
{
  bool failed = false;
  try { operation(); }
  catch (const Standard_Failure&) { failed = true; }
  require(failed);
}

int main()
{
  try
  {
    const Standard_Integer low = (std::numeric_limits<Standard_Integer>::min)();
    const Standard_Integer high = (std::numeric_limits<Standard_Integer>::max)();
    rejected([&] { math_DoubleTab invalid(1, 65536, 1, 65536); });
    rejected([&] { math_DoubleTab invalid(low, high, 1, 1); });
    rejected([&] { math_DoubleTab invalid(5, 1, 1, 1); });
    rejected([&] { math_DoubleTab invalid(nullptr, 1, 1, 1, 1); });
    math_DoubleTab small(-2, 1, 3, 6);
    small.Init(7.0);
    require(small(-2, 3) == 7.0 && small(1, 6) == 7.0);
    math_DoubleTab copied(small);
    small(-2, 3) = 11.0;
    require(copied(-2, 3) == 7.0);
    math_DoubleTab heap(1, 7, 1, 11);
    require(heap.IsDeletable());
    heap.Init(13.0);
    math_DoubleTab heapCopy(heap);
    require(heapCopy(7, 11) == 13.0);
    math_DoubleTab target(1, 7, 1, 11);
    heap.Copy(target);
    require(target(7, 11) == 13.0);
    rejected([&] { heap.Copy(small); });
    require(small(-2, 3) == 11.0);
    Standard_Real storage[9] = {1,2,3,4,5,6,7,8,9};
    math_DoubleTab overlappingSource(storage, 1, 1, 1, 8);
    math_DoubleTab overlappingTarget(storage + 1, 1, 1, 1, 8);
    overlappingSource.Copy(overlappingTarget);
    require(storage[0] == 1 && storage[1] == 1 && storage[8] == 8);
    math_DoubleTab empty(1, 0, 1, 0);
    empty.Init(1.0);
    math_DoubleTab emptyCopy(empty);
    empty.Copy(emptyCopy);
    rejected([&] { empty.SetLowerRow(low); });
    rejected([&] { empty.SetLowerCol(low); });
    math_DoubleTab rebased(low, low + 1, 1, 1);
    rebased.Init(17.0);
    rejected([&] { rebased.SetLowerRow(high); });
    require(rebased(low, 1) == 17.0 && rebased(low + 1, 1) == 17.0);
    rebased.SetLowerRow(2);
    require(rebased(2, 1) == 17.0 && rebased(3, 1) == 17.0);
    math_DoubleTab maximum(high, high, high, high);
    maximum.Init(19.0);
    require(maximum.UpperRow() == high && maximum.UpperCol() == high);
    require(maximum(high, high) == 19.0);
    maximum.SetLowerRow(1);
    maximum.SetLowerCol(1);
    maximum.SetLowerRow(high);
    maximum.SetLowerCol(high);
    require(maximum(high, high) == 19.0);
    math_DoubleTab movedSmall(std::move(small));
    require(movedSmall(-2, 3) == 11.0 && !movedSmall.IsDeletable());
    math_DoubleTab movedExternal(std::move(overlappingTarget));
    require(movedExternal(1, 1) == 1.0 && movedExternal(1, 8) == 8.0);
    movedExternal(1, 1) = 23.0;
    require(storage[1] == 1.0);
    math_DoubleTab movedHeap(std::move(heap));
    require(movedHeap(7, 11) == 13.0 && movedHeap.IsDeletable());
    std::cout << "OCCT checked matrix storage passed; layout bytes=" << sizeof(math_DoubleTab) << '\n';
    return 0;
  }
  catch (const Standard_Failure&)
  {
    std::cerr << "checked OCCT matrix storage unexpectedly rejected valid input\n";
  }
  catch (const std::exception& error)
  {
    std::cerr << error.what() << '\n';
  }
  return 1;
}
