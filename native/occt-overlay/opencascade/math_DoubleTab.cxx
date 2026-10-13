// Copyright (c) 1997-1999 Matra Datavision
// Copyright (c) 1999-2014 OPEN CASCADE SAS
//
// This file is part of Open CASCADE Technology software library.
//
// This library is free software; you can redistribute it and/or modify it under
// the terms of the GNU Lesser General Public License version 2.1 as published
// by the Free Software Foundation, with special exception defined in the file
// OCCT_LGPL_EXCEPTION.txt. Consult the file LICENSE_LGPL_21.txt included in OCCT
// distribution for complete text of the license and disclaimer of any warranty.
//
// Alternatively, this file may be used under the terms of Open CASCADE
// commercial license or contractual agreement.
// Modified for Limo CAD on 2026-10-06: checked matrix storage.

#include <math_DoubleTab.hxx>
#include <Standard_OutOfRange.hxx>

void math_DoubleTab::Allocate()
{
  const std::size_t count = math_DoubleTabChecked::Count(LowR, UppR, LowC, UppC);
  if (isAllocated)
    Addr = Standard::Allocate(count * sizeof(Standard_Real));
  else if (count != 0 && Addr == nullptr)
    Standard_OutOfRange::Raise("math_DoubleTab nonempty storage must not be null");
}

math_DoubleTab::math_DoubleTab(const Standard_Integer LowerRow,
                              const Standard_Integer UpperRow,
                              const Standard_Integer LowerCol,
                              const Standard_Integer UpperCol)
    : Addr(Buf),
      isAllocated(math_DoubleTabChecked::Count(LowerRow, UpperRow, LowerCol, UpperCol) > 16),
      LowR(LowerRow),
      UppR(UpperRow),
      LowC(LowerCol),
      UppC(UpperCol)
{
  Allocate();
}

math_DoubleTab::math_DoubleTab(const Standard_Address Tab,
                              const Standard_Integer LowerRow,
                              const Standard_Integer UpperRow,
                              const Standard_Integer LowerCol,
                              const Standard_Integer UpperCol)
    : Addr(Tab),
      isAllocated(Standard_False),
      LowR(LowerRow),
      UppR(UpperRow),
      LowC(LowerCol),
      UppC(UpperCol)
{
  Allocate();
}

void math_DoubleTab::Init(const Standard_Real InitValue)
{
  const std::size_t count = math_DoubleTabChecked::Count(LowR, UppR, LowC, UppC);
  for (std::size_t index = 0; index < count; ++index)
    ((Standard_Real*)Addr)[index] = InitValue;
}

math_DoubleTab::math_DoubleTab(const math_DoubleTab& Other)
    : Addr(Buf),
      isAllocated(math_DoubleTabChecked::Count(Other.LowR, Other.UppR, Other.LowC, Other.UppC) > 16),
      LowR(Other.LowR),
      UppR(Other.UppR),
      LowC(Other.LowC),
      UppC(Other.UppC)
{
  Allocate();
  Other.Copy(*this);
}

void math_DoubleTab::Free()
{
  if (isAllocated)
    Standard::Free(Addr);
  Addr = nullptr;
}

void math_DoubleTab::SetLowerRow(const Standard_Integer LowerRow)
{
  UppR = math_DoubleTabChecked::Rebase(LowR, UppR, LowerRow);
  LowR = LowerRow;
}

void math_DoubleTab::SetLowerCol(const Standard_Integer LowerCol)
{
  UppC = math_DoubleTabChecked::Rebase(LowC, UppC, LowerCol);
  LowC = LowerCol;
}
