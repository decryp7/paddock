/** Every ISO 3166-1 alpha-2 code a location database can return, plus Kosovo's user-assigned `XK` that DB-IP uses. */
export const ISO_CODES: readonly string[] =
  'AD AE AF AG AI AL AM AO AQ AR AS AT AU AW AX AZ BA BB BD BE BF BG BH BI BJ BL BM BN BO BQ BR BS BT BV BW BY BZ CA CC CD CF CG CH CI CK CL CM CN CO CR CU CV CW CX CY CZ DE DJ DK DM DO DZ EC EE EG EH ER ES ET FI FJ FK FM FO FR GA GB GD GE GF GG GH GI GL GM GN GP GQ GR GS GT GU GW GY HK HM HN HR HT HU ID IE IL IM IN IO IQ IR IS IT JE JM JO JP KE KG KH KI KM KN KP KR KW KY KZ LA LB LC LI LK LR LS LT LU LV LY MA MC MD ME MF MG MH MK ML MM MN MO MP MQ MR MS MT MU MV MW MX MY MZ NA NC NE NF NG NI NL NO NP NR NU NZ OM PA PE PF PG PH PK PL PM PN PR PS PT PW PY QA RE RO RS RU RW SA SB SC SD SE SG SH SI SJ SK SL SM SN SO SR SS ST SV SX SY SZ TC TD TF TG TH TJ TK TL TM TN TO TR TT TV TW TZ UA UG UM US UY UZ VA VC VE VG VI VN VU WF WS XK YE YT ZA ZM ZW'.split(
    ' ',
  );

/**
 * Countries and territories too small for the 1:110m map: a dot at [longitude, latitude] (approximate: the place, not a
 * boundary). Every code in {@link ISO_CODES} has a shape in `COUNTRY_PATHS` or an entry here (a spec checks it).
 */
export const SMALL_PLACES: Readonly<Record<string, readonly [number, number]>> = {
  AD: [1.5, 42.5], AG: [-61.8, 17.1], AI: [-63.1, 18.2], AS: [-170.7, -14.3], AW: [-70.0, 12.5], AX: [20.0, 60.2],
  BB: [-59.5, 13.2], BH: [50.6, 26.0], BL: [-62.8, 17.9], BM: [-64.8, 32.3], BQ: [-68.3, 12.2], BV: [3.4, -54.4],
  CC: [96.8, -12.2], CK: [-159.8, -21.2], CV: [-23.6, 15.1], CW: [-69.0, 12.2], CX: [105.7, -10.5], DM: [-61.4, 15.4],
  FM: [158.2, 6.9], FO: [-6.9, 62.0], GD: [-61.7, 12.1], GF: [-53.1, 3.9], GG: [-2.6, 49.5], GI: [-5.35, 36.1],
  GP: [-61.6, 16.2], GS: [-36.5, -54.4], GU: [144.8, 13.4], HK: [114.2, 22.3], HM: [73.5, -53.1], IM: [-4.5, 54.2],
  IO: [72.4, -7.3], JE: [-2.1, 49.2], KI: [173.0, 1.4], KM: [43.9, -11.9], KN: [-62.7, 17.3], KY: [-81.3, 19.3],
  LC: [-61.0, 13.9], LI: [9.55, 47.15], MC: [7.4, 43.7], MF: [-63.05, 18.07], MH: [171.2, 7.1], MO: [113.5, 22.2],
  MP: [145.7, 15.2], MQ: [-61.0, 14.6], MS: [-62.2, 16.7], MT: [14.4, 35.9], MU: [57.6, -20.3], MV: [73.5, 3.2],
  NF: [167.95, -29.0], NR: [166.9, -0.5], NU: [-169.9, -19.05], PF: [-149.4, -17.7], PM: [-56.3, 46.9], PN: [-128.3, -24.4],
  PW: [134.6, 7.5], RE: [55.5, -21.1], SC: [55.5, -4.7], SG: [103.8, 1.35], SH: [-5.7, -15.95], SJ: [15.6, 78.2],
  SM: [12.45, 43.9], ST: [6.6, 0.3], SX: [-63.05, 18.04], TC: [-71.8, 21.8], TK: [-171.8, -9.2], TO: [-175.2, -21.2],
  TV: [178.7, -7.5], UM: [-176.6, 0.2], VA: [12.45, 41.9], VC: [-61.2, 13.2], VG: [-64.6, 18.4], VI: [-64.9, 18.3],
  WF: [-176.2, -13.3], WS: [-172.1, -13.8], YT: [45.2, -12.8],
};
