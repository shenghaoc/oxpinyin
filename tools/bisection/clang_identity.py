"""Small read-only libclang adapter for the J ledger (no Python clang dependency)."""

import ctypes as C
import ctypes.util
from pathlib import Path


class Cursor(C.Structure):
    _fields_ = [("kind", C.c_uint), ("xdata", C.c_int), ("data", C.c_void_p * 3)]


class String(C.Structure):
    _fields_ = [("data", C.c_void_p), ("flags", C.c_uint)]


class Location(C.Structure):
    _fields_ = [("data", C.c_void_p * 2), ("int_data", C.c_uint)]


class Type(C.Structure):
    _fields_ = [("kind", C.c_int), ("data", C.c_void_p * 2)]


class Range(C.Structure):
    _fields_ = [("data", C.c_void_p * 2), ("begin", C.c_uint), ("end", C.c_uint)]


class Clang:
    def __init__(self, library=None):
        self.lib = C.CDLL(library or C.util.find_library("clang"))
        self.visit_type = C.CFUNCTYPE(C.c_uint, Cursor, Cursor, C.c_void_p)
        signatures = {
            "createIndex": (C.c_void_p, [C.c_int, C.c_int]),
            "disposeIndex": (None, [C.c_void_p]),
            "parseTranslationUnit": (C.c_void_p, [C.c_void_p, C.c_char_p, C.POINTER(C.c_char_p), C.c_int, C.c_void_p, C.c_uint, C.c_uint]),
            "disposeTranslationUnit": (None, [C.c_void_p]),
            "getTranslationUnitCursor": (Cursor, [C.c_void_p]),
            "visitChildren": (C.c_uint, [Cursor, self.visit_type, C.c_void_p]),
            "getCursorUSR": (String, [Cursor]),
            "getCursorSpelling": (String, [Cursor]),
            "getCursorDisplayName": (String, [Cursor]),
            "Cursor_getMangling": (String, [Cursor]),
            "getCursorReferenced": (Cursor, [Cursor]),
            "getCursorDefinition": (Cursor, [Cursor]),
            "getSpecializedCursorTemplate": (Cursor, [Cursor]),
            "getCursorSemanticParent": (Cursor, [Cursor]),
            "getCursorType": (Type, [Cursor]),
            "getTypeSpelling": (String, [Type]),
            "getCanonicalType": (Type, [Type]),
            "getPointeeType": (Type, [Type]),
            "getTypeDeclaration": (Cursor, [Type]),
            "getCursorLocation": (Location, [Cursor]),
            "getCursorExtent": (Range, [Cursor]),
            "getRangeStart": (Location, [Range]),
            "getRangeEnd": (Location, [Range]),
            "getSpellingLocation": (None, [Location, C.POINTER(C.c_void_p), C.POINTER(C.c_uint), C.POINTER(C.c_uint), C.POINTER(C.c_uint)]),
            "getFileName": (String, [C.c_void_p]),
            "getCString": (C.c_char_p, [String]),
            "disposeString": (None, [String]),
            "isCursorDefinition": (C.c_uint, [Cursor]),
            "CXXMethod_isVirtual": (C.c_uint, [Cursor]),
            "CXXMethod_isStatic": (C.c_uint, [Cursor]),
            "CXXMethod_isPureVirtual": (C.c_uint, [Cursor]),
            "CXXRecord_isAbstract": (C.c_uint, [Cursor]),
            "getOverriddenCursors": (None, [Cursor, C.POINTER(C.POINTER(Cursor)), C.POINTER(C.c_uint)]),
            "disposeOverriddenCursors": (None, [C.POINTER(Cursor)]),
            "getNumDiagnostics": (C.c_uint, [C.c_void_p]),
            "getDiagnostic": (C.c_void_p, [C.c_void_p, C.c_uint]),
            "getDiagnosticSeverity": (C.c_uint, [C.c_void_p]),
            "formatDiagnostic": (String, [C.c_void_p, C.c_uint]),
            "disposeDiagnostic": (None, [C.c_void_p]),
        }
        for name, (result, args) in signatures.items():
            fn = getattr(self.lib, "clang_" + name)
            fn.restype, fn.argtypes = result, args
            setattr(self, name, fn)

    def string(self, value):
        result = self.getCString(value)
        text = result.decode() if result else ""
        self.disposeString(value)
        return text

    def location(self, cursor):
        return self.source_location(self.getCursorLocation(cursor))

    def source_location(self, location):
        file, line, col, offset = C.c_void_p(), C.c_uint(), C.c_uint(), C.c_uint()
        self.getSpellingLocation(location, C.byref(file), C.byref(line), C.byref(col), C.byref(offset))
        return (self.string(self.getFileName(file)) if file.value else "", line.value, col.value)

    def children(self, cursor):
        result = []

        @self.visit_type
        def visit(child, parent, context):
            result.append(child)
            return 1

        self.visitChildren(cursor, visit, None)
        return result

    def extract(self, source, flags, pin):
        """Return source definitions, exact USR calls, override relations and fields."""
        index = self.createIndex(0, 0)
        argv = (C.c_char_p * len(flags))(*(x.encode() for x in flags))
        tu = self.parseTranslationUnit(index, str(source).encode(), argv, len(flags), None, 0, 0)
        if not tu:
            raise RuntimeError(f"libclang could not parse {source.name}")
        try:
            for i in range(self.getNumDiagnostics(tu)):
                d = self.getDiagnostic(tu, i)
                if self.getDiagnosticSeverity(d) >= 3:
                    message = self.string(self.formatDiagnostic(d, 0))
                    self.disposeDiagnostic(d)
                    raise RuntimeError(message)
                self.disposeDiagnostic(d)
            nodes, edges, virtual, overrides, fields = {}, set(), set(), set(), []
            metadata = {"bases": set(), "virtual_sites": set(), "deletes": set(), "abstract_classes": set()}
            function_kinds = {8, 21, 24, 25, 26, 30}

            def info(c):
                usr = self.string(self.getCursorUSR(c))
                file, line, col = self.location(c)
                try:
                    file = str(Path(file).relative_to(pin))
                except ValueError:
                    return usr, None
                parents, parent = [], self.getCursorSemanticParent(c)
                while parent.kind not in {350, 70}:
                    name = self.string(self.getCursorSpelling(parent))
                    if name:
                        parents.append(name)
                    parent = self.getCursorSemanticParent(parent)
                name = self.string(self.getCursorDisplayName(c))
                signature = "::".join(list(reversed(parents)) + [name]) + " : " + self.string(self.getTypeSpelling(self.getCursorType(c)))
                template = self.getSpecializedCursorTemplate(c)
                primary_owner = self.getSpecializedCursorTemplate(self.getCursorSemanticParent(c))
                return usr, {"usr": usr, "signature": signature, "file": file, "line": line, "col": col, "symbol": self.string(self.Cursor_getMangling(c)), "name": self.string(self.getCursorSpelling(c)), "owner_usr": self.string(self.getCursorUSR(self.getCursorSemanticParent(c))), "canonical_type": self.string(self.getTypeSpelling(self.getCanonicalType(self.getCursorType(c)))), "function_kind": c.kind, "template_usr": self.string(self.getCursorUSR(template)), "template_owner_usr": self.string(self.getCursorUSR(primary_owner)), "is_static": bool(self.CXXMethod_isStatic(c)) if c.kind == 21 else False}

            def type_class(c):
                typ = self.getCanonicalType(self.getCursorType(c))
                if typ.kind in {101, 103, 104}:  # pointer/lvalue/rvalue reference
                    typ = self.getCanonicalType(self.getPointeeType(typ))
                return self.getTypeDeclaration(typ)

            def receiver_class(call):
                children = self.children(call)
                if not children:
                    return ""
                callee = children[0]
                while callee.kind == 100 and self.children(callee):
                    callee = self.children(callee)[0]
                if callee.kind != 102:
                    return ""
                operands = self.children(callee)
                if not operands:
                    return ""
                return self.string(self.getCursorUSR(type_class(operands[0])))

            def walk(c, current=None, owner=None):
                file, _, _ = self.location(c)
                if file and not Path(file).is_relative_to(pin):
                    return
                if c.kind in {2, 4, 31}:
                    owner = self.string(self.getCursorUSR(c))
                    if self.CXXRecord_isAbstract(c):
                        metadata["abstract_classes"].add(owner)
                if c.kind == 44 and owner:
                    metadata["bases"].add((owner, self.string(self.getCursorUSR(type_class(c)))))
                if c.kind in function_kinds:
                    usr, node = info(c)
                    if node:
                        isdef = self.isCursorDefinition(c)
                        node["definition"] = bool(isdef)
                        if isdef:
                            bodies = [child for child in self.children(c) if child.kind == 202]
                            if len(bodies) == 1:
                                extent = self.getCursorExtent(bodies[0])
                                start = self.source_location(self.getRangeStart(extent))
                                end = self.source_location(self.getRangeEnd(extent))
                                if start[0] == file and end[0] == file:
                                    node["body_start"], node["body_end"] = start[1], end[1]
                        if isdef or usr not in nodes:
                            nodes[usr] = node
                        if c.kind in {21, 25} and self.CXXMethod_isVirtual(c):
                            if not self.CXXMethod_isPureVirtual(c):
                                virtual.add(usr)
                            array, count = C.POINTER(Cursor)(), C.c_uint()
                            self.getOverriddenCursors(c, C.byref(array), C.byref(count))
                            for j in range(count.value):
                                overrides.add((self.string(self.getCursorUSR(array[j])), usr))
                            self.disposeOverriddenCursors(array)
                        if isdef:
                            current = usr
                if c.kind == 103 and current:
                    ref = self.getCursorReferenced(c)
                    usr, node = info(ref)
                    if ref.kind in function_kinds and node:
                        if usr not in nodes:
                            nodes[usr] = node
                        isvirtual = ref.kind in {21, 25} and self.CXXMethod_isVirtual(ref)
                        edges.add((current, usr, "virtual" if isvirtual else "AST-direct", self.location(c)[1]))
                        if isvirtual:
                            metadata["virtual_sites"].add((current, self.location(c)[1], usr, receiver_class(c)))
                    elif ref.kind == 6:
                        parent = self.string(self.getCursorSpelling(self.getCursorSemanticParent(ref)))
                        fields.append((current, self.location(c)[1], parent, self.string(self.getCursorSpelling(ref)),
                                       self.string(self.getCursorUSR(ref)), self.string(self.getTypeSpelling(self.getCursorType(ref))), Path(self.location(ref)[0]).name))
                if c.kind == 135 and current:  # CXXDeleteExpr: destructor dispatch
                    operands = self.children(c)
                    if operands:
                        cls = type_class(operands[0])
                        receiver = self.string(self.getCursorUSR(cls))
                        definition = self.getCursorDefinition(cls)
                        if definition.kind not in {70, 71, 72, 73}:
                            cls = definition
                        for child in self.children(cls):
                            if child.kind != 25:
                                continue
                            usr, node = info(child)
                            if not node:
                                continue
                            nodes.setdefault(usr, node)
                            kind = "virtual" if self.CXXMethod_isVirtual(child) else "AST-direct"
                            edges.add((current, usr, kind, self.location(c)[1]))
                            metadata["deletes"].add((current, self.location(c)[1], usr, receiver))
                            if kind == "virtual":
                                metadata["virtual_sites"].add((current, self.location(c)[1], usr, receiver))
                for child in self.children(c):
                    walk(child, current, owner)

            walk(self.getTranslationUnitCursor(tu))
            return nodes, edges, virtual, overrides, fields, metadata
        finally:
            self.disposeTranslationUnit(tu)
            self.disposeIndex(index)
