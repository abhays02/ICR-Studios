"""Literal evidence is immutable; normalization and rule issues are separate."""
import re
from datetime import date, datetime
from decimal import Decimal, InvalidOperation


NUMERIC_MAP = str.maketrans({"O": "0", "I": "1", "l": "1", "S": "5", "B": "8"})
ONES = dict(zip(("zero one two three four five six seven eight nine ten eleven twelve "
                 "thirteen fourteen fifteen sixteen seventeen eighteen nineteen").split(), range(20)))
TENS = dict(zip("twenty thirty forty fifty sixty seventy eighty ninety".split(), range(20, 100, 10)))
SCALES = {"thousand": 1000, "lakh": 100000, "lakhs": 100000,
          "crore": 10000000, "crores": 10000000, "million": 1000000}


def amount_figures(text):
    if text is None or not text.strip():
        return None, "unknown"
    value = re.sub(r"^(?:₹|Rs\.?|INR)\s*", "", text.strip(), flags=re.I)
    if not re.fullmatch(r"(?:\d{1,15}|\d{1,3}(?:,\d{3})+|\d{1,2}(?:,\d{2})*,\d{3})(?:\.\d{1,2})?", value):
        return None, "unparseable"
    try:
        amount = Decimal(value.replace(",", ""))
        if not Decimal(0) <= amount <= Decimal("999999999999999.99"):
            return None, "unparseable"
        return amount, "parsed"
    except InvalidOperation:
        return None, "unparseable"


def _word_integer(words):
    if not words or words[0] == "and" or words[-1] == "and":
        raise ValueError()
    total, group, previous_scale = 0, 0, 10**16
    last_small = False
    for word in words:
        if word == "and":
            continue
        if word in ONES or word in TENS:
            number = ONES.get(word, TENS.get(word))
            if last_small and not (group % 100 in TENS.values() and 0 < number < 10):
                raise ValueError()
            group += number
            last_small = True
        elif word == "hundred" and 0 < group < 10:
            group *= 100
            last_small = False
        elif word in SCALES and 0 < group and SCALES[word] < previous_scale:
            previous_scale = SCALES[word]
            total += group * previous_scale
            group, last_small = 0, False
        else:
            raise ValueError()
    return total + group


def amount_words(text):
    if text is None or not text.strip():
        return None, "unknown"
    words = text.lower().replace("-", " ").split()
    words = [w for w in words if w not in {"rupees", "rupee", "inr", "only"}]
    try:
        if not words:
            raise ValueError()
        if "paise" in words or "paisa" in words:
            if words[-1] not in {"paise", "paisa"} or "and" not in words:
                raise ValueError()
            split = len(words) - 1 - words[::-1].index("and")
            whole, fraction = _word_integer(words[:split]), _word_integer(words[split + 1:-1])
            if not 0 <= fraction < 100:
                raise ValueError()
            value = Decimal(whole) + Decimal(fraction) / 100
        elif "point" in words:
            split = words.index("point")
            digits = words[split + 1:]
            if not 1 <= len(digits) <= 2 or any(w not in ONES or ONES[w] > 9 for w in digits):
                raise ValueError()
            value = Decimal(f"{_word_integer(words[:split])}." + "".join(str(ONES[w]) for w in digits))
        else:
            value = Decimal(_word_integer(words))
        return (value, "parsed") if value <= Decimal("999999999999999.99") else (None, "unparseable")
    except (ValueError, InvalidOperation):
        return None, "unparseable"


def normalize(raw, spec):
    value, transformations, issues = raw, [], []
    kind = spec["type"]
    if raw is None:
        return None, [], ["required"] if spec.get("required") else []
    if kind == "checkbox":
        if type(raw) is bool:
            return raw, [], []
        return None, [], ["manual_checkbox_required"]
    if not isinstance(raw, str):
        return None, [], ["invalid_type"]
    value = raw.strip()
    if value != raw:
        transformations.append("trim")
    if spec.get("numeric") is True:
        mapped = value.translate(NUMERIC_MAP)
        if mapped != value:
            transformations.append("numeric_box_visual_substitution")
        value = mapped
        if not value.isascii() or not value.isdigit():
            issues.append("numeric_box_requires_digits")
    if value == "":
        return "", transformations, ["required"] if spec.get("required") else []
    fid = spec["field_id"].lower()
    if kind == "ifsc" or fid == "ifsc":
        if not re.fullmatch(r"[A-Z]{4}0[A-Z0-9]{6}", value):
            issues.append("invalid_ifsc")
    if kind == "pan" or fid in ("pan", "tax_identifier"):
        if not re.fullmatch(r"[A-Z]{5}[0-9]{4}[A-Z]", value):
            issues.append("invalid_pan")
    if kind == "date":
        try:
            if re.fullmatch(r"\d{4}-\d{2}-\d{2}", value):
                parsed = date.fromisoformat(value)
            elif re.fullmatch(r"\d{2}/\d{2}/\d{4}", value):
                parsed = datetime.strptime(value, "%d/%m/%Y").date()
                if parsed.day <= 12 and parsed.month <= 12 and parsed.day != parsed.month:
                    issues.append("ambiguous_date_order")
            else:
                raise ValueError()
            value = parsed.isoformat()
            transformations.append("calendar_date_iso")
            if fid == "date_of_birth" and parsed > date.today():
                issues.append("future_date_of_birth")
        except ValueError:
            issues.append("invalid_calendar_date")
    if kind in ("decimal", "amount_figures"):
        number, status = amount_figures(value)
        if number is None:
            issues.append("amount_" + status)
        else:
            value = format(number, ".2f")
            transformations.append("decimal_string")
    if kind == "amount_words" and amount_words(value)[0] is None:
        issues.append("amount_words_unparseable")
    if spec.get("choices") and value not in spec["choices"]:
        issues.append("invalid_choice")
    return value, transformations, issues


def reconcile(fields, specs):
    words_id = next((s["field_id"] for s in specs if s["type"] == "amount_words" or s["field_id"] == "amount_words"), None)
    figures_id = next((s["field_id"] for s in specs if s["type"] == "amount_figures" or s["field_id"] == "amount_figures"), None)
    if not words_id or not figures_id:
        return {"status": "unknown", "match": None}
    words, ws = amount_words(fields.get(words_id, {}).get("value"))
    figures, fs = amount_figures(fields.get(figures_id, {}).get("value"))
    if words is None or figures is None:
        return {"status": "unparseable" if "unparseable" in (ws, fs) else "unknown", "match": None}
    return {"status": "match" if words == figures else "mismatch", "match": words == figures}
