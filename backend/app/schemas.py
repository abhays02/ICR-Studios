import re
from typing import Literal

from pydantic import BaseModel, ConfigDict, Field, model_validator


class Strict(BaseModel):
    model_config = ConfigDict(extra="forbid", strict=True)


Box = list[int]


class FieldSpec(Strict):
    field_id: str = Field(pattern=r"^[a-zA-Z][a-zA-Z0-9_]{0,63}$")
    label: str = Field(min_length=1, max_length=160)
    type: Literal["text", "multiline", "enum", "decimal", "checkbox", "date", "comb",
                  "ifsc", "pan", "amount_words", "amount_figures"] = "text"
    required: bool = False
    page: Literal[1] = 1
    roi: Box
    line_rois: list[Box] = Field(default_factory=list, max_length=100)
    cells: list[Box] = Field(default_factory=list, max_length=32)
    numeric: bool = False
    choices: list[str] = Field(default_factory=list, max_length=100)


class Template(Strict):
    template_id: str = Field(pattern=r"^[a-zA-Z0-9_-]{1,64}$")
    template_version: str = Field(pattern=r"^[a-zA-Z0-9_.-]{1,32}$")
    schema_version: Literal["1.0.0"] = "1.0.0"
    form_kind: str = Field(min_length=1, max_length=32)
    page: Literal[1] = 1
    width_px: int = Field(ge=1, le=25000)
    height_px: int = Field(ge=1, le=25000)
    coordinate_space: Literal["template_pixels"] = "template_pixels"
    bbox_convention: Literal["half_open_xyxy"] = "half_open_xyxy"
    synthetic: bool = False
    fields: list[FieldSpec] = Field(min_length=1, max_length=100)

    @model_validator(mode="after")
    def geometry(self):
        if self.width_px * self.height_px > 25000000:
            raise ValueError("Template pixel limit")
        if len({f.field_id for f in self.fields}) != len(self.fields):
            raise ValueError("Duplicate field IDs")
        if sum(max(1, len(f.line_rois), len(f.cells)) for f in self.fields) > 100:
            raise ValueError("Maximum 100 inference crops")
        for f in self.fields:
            for box in [f.roi] + f.line_rois + f.cells:
                if len(box) != 4 or any(type(x) is not int for x in box):
                    raise ValueError("Integer half-open xyxy required")
                x0, y0, x1, y1 = box
                if not (0 <= x0 < x1 <= self.width_px and 0 <= y0 < y1 <= self.height_px):
                    raise ValueError("ROI outside image")
            for box in f.line_rois + f.cells:
                if not (f.roi[0] <= box[0] < box[2] <= f.roi[2]
                        and f.roi[1] <= box[1] < box[3] <= f.roi[3]):
                    raise ValueError("Subcrop outside field ROI")
            if f.cells and (f.type != "comb" or f.line_rois):
                raise ValueError("Comb cells and line ROIs are mutually exclusive")
        return self


class JobRequest(Strict):
    idempotency_key: str = Field(min_length=8, max_length=128, pattern=r"^[A-Za-z0-9_-]+$")
    alignment_confirmed: Literal[True]
    remote_consent: Literal[False] = False
    rotation: Literal[0] = 0
    deskew: Literal[False] = False
    denoise: Literal[False] = False


class Correction(Strict):
    revision: int = Field(ge=1)
    value: str | bool | None

    @model_validator(mode="after")
    def bounded(self):
        if isinstance(self.value, str) and (len(self.value) > 4096 or
                                           re.search(r"[\x00-\x08\x0b\x0c\x0e-\x1f]", self.value)):
            raise ValueError("Invalid field text")
        return self


class Revision(Strict):
    revision: int = Field(ge=1)


class SettingsPatch(Strict):
    revision: int = Field(ge=0)
    values: dict
