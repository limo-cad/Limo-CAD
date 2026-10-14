// Created on: 1992-02-07
// Created by: Laurent PAINNOT
// Copyright (c) 1992-1999 Matra Datavision
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

#ifndef _math_DoubleTab_HeaderFile
#define _math_DoubleTab_HeaderFile

#include <Standard.hxx>
#include <Standard_DefineAlloc.hxx>
#include <Standard_Handle.hxx>
#include <NCollection_Array2.hxx>
#include <Standard_DimensionMismatch.hxx>
#include <Standard_OutOfRange.hxx>

#include <Standard_Real.hxx>
#include <Standard_Boolean.hxx>

#include <array>
#include <cstdint>
#include <cstring>
#include <limits>
#include <utility>

class math_DoubleTab
{
  static const int THE_BUFFER_SIZE = 64;

public:
  DEFINE_STANDARD_ALLOC;
  DEFINE_NCOLLECTION_ALLOC;

public:
  //! Constructor for ranges [theLowerRow..theUpperRow, theLowerCol..theUpperCol]
  math_DoubleTab(const int theLowerRow,
                 const int theUpperRow,
                 const int theLowerCol,
                 const int theUpperCol)
      : myBuffer{},
        myArray(MakeArray(myBuffer.data(), theLowerRow, theUpperRow, theLowerCol, theUpperCol, false))
  {
  }

public:
  //! Constructor from external data array
  math_DoubleTab(void* const theTab,
                 const int   theLowerRow,
                 const int   theUpperRow,
                 const int   theLowerCol,
                 const int   theUpperCol)
      : myBuffer{},
        myArray(MakeArray(static_cast<double*>(theTab),
                          theLowerRow, theUpperRow, theLowerCol, theUpperCol, true))
  {
  }

  //! Initialize all elements with theInitValue
  void Init(const double theInitValue) { myArray.Init(theInitValue); }

  //! Copy constructor
  math_DoubleTab(const math_DoubleTab& theOther)
      : myArray(theOther.myArray)
  {
  }

  //! Move constructor
  math_DoubleTab(math_DoubleTab&& theOther) noexcept
      : myBuffer{},
        myArray(theOther.myArray.IsDeletable()
                  ? std::move(theOther.myArray)
                  : MakeArray(myBuffer.data(), theOther.LowerRow(), theOther.UpperRow(),
                              theOther.LowerCol(), theOther.UpperCol(), false))
  {
    if (!theOther.myArray.IsEmpty())
    {
      theOther.Copy(*this);
    }
  }

  //! Copy data to theOther
  void Copy(math_DoubleTab& theOther) const
  {
    if (NbRows() != theOther.NbRows() || NbColumns() != theOther.NbColumns())
      throw Standard_DimensionMismatch("math_DoubleTab copy dimensions differ");
    const std::size_t count = myArray.Size();
    if (count != 0)
      std::memmove(&theOther.myArray.ChangeAt(0, 0), &myArray.At(0, 0), count * sizeof(double));
  }

  //! Returns true if the internal array is deletable (heap-allocated)
  bool IsDeletable() const { return myArray.IsDeletable(); }

  //! Set lower row index
  void SetLowerRow(const int theLowerRow)
  {
    CheckedUpper(theLowerRow, NbRows());
    myArray.UpdateLowerRow(theLowerRow);
  }

  //! Set lower column index
  void SetLowerCol(const int theLowerCol)
  {
    CheckedUpper(theLowerCol, NbColumns());
    myArray.UpdateLowerCol(theLowerCol);
  }

  //! Get lower row index
  int LowerRow() const noexcept { return myArray.LowerRow(); }

  //! Get upper row index
  int UpperRow() const noexcept { return static_cast<int>(std::int64_t(LowerRow()) + NbRows() - 1); }

  //! Get lower column index
  int LowerCol() const noexcept { return myArray.LowerCol(); }

  //! Get upper column index
  int UpperCol() const noexcept { return static_cast<int>(std::int64_t(LowerCol()) + NbColumns() - 1); }

  //! Get number of rows
  int NbRows() const noexcept { return myArray.NbRows(); }

  //! Get number of columns
  int NbColumns() const noexcept { return myArray.NbColumns(); }

  //! Access element at (theRowIndex, theColIndex)
  const double& Value(const int theRowIndex, const int theColIndex) const
  {
    return myArray.At(static_cast<std::size_t>(std::int64_t(theRowIndex) - LowerRow()),
                      static_cast<std::size_t>(std::int64_t(theColIndex) - LowerCol()));
  }

  //! Change element at (theRowIndex, theColIndex)
  double& Value(const int theRowIndex, const int theColIndex)
  {
    return myArray.ChangeAt(static_cast<std::size_t>(std::int64_t(theRowIndex) - LowerRow()),
                            static_cast<std::size_t>(std::int64_t(theColIndex) - LowerCol()));
  }

  //! Operator() - alias to Value
  const double& operator()(const int theRowIndex, const int theColIndex) const
  {
    return Value(theRowIndex, theColIndex);
  }

  //! Operator() - alias to ChangeValue
  double& operator()(const int theRowIndex, const int theColIndex)
  {
    return Value(theRowIndex, theColIndex);
  }

  //! Assignment operator
  math_DoubleTab& operator=(const math_DoubleTab& theOther)
  {
    if (this != &theOther)
    {
      myArray = theOther.myArray;
    }
    return *this;
  }

  //! Move assignment operator
  math_DoubleTab& operator=(math_DoubleTab&& theOther) noexcept
  {
    if (this == &theOther)
    {
      return *this;
    }

    if (myArray.IsDeletable() && theOther.myArray.IsDeletable()
        && myArray.NbRows() == theOther.myArray.NbRows()
        && myArray.NbColumns() == theOther.myArray.NbColumns()
        && myArray.LowerRow() == theOther.myArray.LowerRow()
        && myArray.LowerCol() == theOther.myArray.LowerCol())
    {
      myArray.Move(theOther.myArray);
    }
    else
    {
      myArray = theOther.myArray;
    }
    return *this;
  }

  //! Destructor
  ~math_DoubleTab() = default;

private:
  static std::size_t Extent(int lower, int upper)
  {
    const std::int64_t extent = std::int64_t(upper) - lower + 1;
    if (extent < 0 || extent > (std::numeric_limits<int>::max)())
      throw Standard_OutOfRange("math_DoubleTab dimension is outside its index range");
    return static_cast<std::size_t>(extent);
  }

  static void CheckedUpper(int lower, int extent)
  {
    const std::int64_t upper = std::int64_t(lower) + extent - 1;
    if (upper < (std::numeric_limits<int>::min)() || upper > (std::numeric_limits<int>::max)())
      throw Standard_OutOfRange("math_DoubleTab rebased dimension is outside its index range");
  }

  static NCollection_Array2<double> MakeArray(double* buffer,
                                             int lowerRow, int upperRow,
                                             int lowerCol, int upperCol,
                                             bool external)
  {
    const std::size_t rows = Extent(lowerRow, upperRow);
    const std::size_t cols = Extent(lowerCol, upperCol);
    if (cols != 0 && rows > static_cast<std::size_t>((std::numeric_limits<int>::max)()) / cols)
      throw Standard_OutOfRange("math_DoubleTab storage exceeds its index range");
    const std::size_t count = rows * cols;
    if (count > (std::numeric_limits<std::size_t>::max)() / sizeof(double))
      throw Standard_OutOfRange("math_DoubleTab storage exceeds its byte range");
    if (count != 0 && buffer == nullptr)
      throw Standard_OutOfRange("math_DoubleTab nonempty storage must not be null");
    // Avoid Array2's legacy int-bound constructors and their signed offset arithmetic.
    NCollection_Array2<double> array = external || count <= THE_BUFFER_SIZE
      ? NCollection_Array2<double>(buffer, rows, cols)
      : NCollection_Array2<double>(rows, cols);
    array.UpdateLowerRow(lowerRow);
    array.UpdateLowerCol(lowerCol);
    return array;
  }

  std::array<double, THE_BUFFER_SIZE> myBuffer;
  NCollection_Array2<double>          myArray;
};

#endif // _math_DoubleTab_HeaderFile
